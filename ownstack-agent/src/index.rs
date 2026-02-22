use candle_core::{Device, Tensor};
use candle_transformers::models::bert::{BertModel, Config};
use hnsw_rs::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tokenizers::Tokenizer;
use tracing::{debug, info};

const INDEX_SCHEMA_VERSION: u32 = 1;
const INDEX_EMBEDDING_DIM: usize = 384;
const INDEX_MODEL_ID: &str = "bert-embeddings-v1";
const HNSW_MAX_CONNECTIONS: usize = 32;
const HNSW_MAX_ELEMENTS: usize = 100_000;
const HNSW_MAX_LAYERS: usize = 16;
const HNSW_EF_CONSTRUCTION: usize = 200;
const CHUNK_LINES: usize = 20;

pub struct SemanticIndex {
    workspace: PathBuf,
    model: Option<BertModel>,
    tokenizer: Option<Tokenizer>,
    device: Device,
    index_store: Arc<RwLock<Option<Hnsw<'static, f32, DistCosine>>>>,
    metadata: Arc<RwLock<Vec<ChunkMetadata>>>,
    embeddings: Arc<RwLock<Vec<Vec<f32>>>>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ChunkMetadata {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub content: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
struct IndexManifest {
    schema_version: u32,
    model_id: String,
    embedding_dim: usize,
    entry_count: usize,
    checksum_sha256: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
struct PersistedIndexData {
    embeddings: Vec<Vec<f32>>,
    metadata: Vec<ChunkMetadata>,
}

#[derive(Clone)]
struct CachedChunkEntry {
    metadata: ChunkMetadata,
    embedding: Vec<f32>,
}

impl SemanticIndex {
    pub fn new(workspace: PathBuf) -> Self {
        let device = Device::Cpu;
        Self {
            workspace,
            model: None,
            tokenizer: None,
            device,
            index_store: Arc::new(RwLock::new(None)),
            metadata: Arc::new(RwLock::new(Vec::new())),
            embeddings: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn init(&mut self) -> Result<(), String> {
        if self.model.is_some() {
            return Ok(());
        }

        let model_dir = self.workspace.join(".ownstack").join("models");
        if !model_dir.exists() {
            return Err(
                "Model directory not found. Please run bootstrap.".to_string()
            );
        }

        info!("Loading BERT model from {:?}", model_dir);

        let config_filename = model_dir.join("config.json");
        let tokenizer_filename = model_dir.join("tokenizer.json");
        let weights_filename = model_dir.join("model.safetensors");

        let config_str = std::fs::read_to_string(&config_filename)
            .map_err(|e| format!("Failed to read BERT config: {}", e))?;
        let config: Config = serde_json::from_str(&config_str)
            .map_err(|e| format!("Failed to parse BERT config: {}", e))?;

        let tokenizer = Tokenizer::from_file(tokenizer_filename)
            .map_err(|e| format!("Failed to load tokenizer: {}", e))?;
        let vb = {
            let tensors =
                candle_core::safetensors::load(&weights_filename, &self.device)
                    .map_err(|e| format!("Failed to load model weights: {}", e))?;
            candle_nn::VarBuilder::from_tensors(
                tensors,
                candle_core::DType::F32,
                &self.device,
            )
        };

        let model = BertModel::load(vb, &config)
            .map_err(|e| format!("Failed to initialize BERT model: {}", e))?;

        self.model = Some(model);
        self.tokenizer = Some(tokenizer);

        if let Err(err) = self.load().await {
            debug!(
                "No existing index loaded ({}). Initializing fresh semantic index.",
                err
            );
            let hnsw = Hnsw::new(
                HNSW_MAX_CONNECTIONS,
                HNSW_MAX_ELEMENTS,
                HNSW_MAX_LAYERS,
                HNSW_EF_CONSTRUCTION,
                DistCosine {},
            );
            *self.index_store.write().map_err(|e| e.to_string())? = Some(hnsw);
            *self.metadata.write().map_err(|e| e.to_string())? = Vec::new();
            *self.embeddings.write().map_err(|e| e.to_string())? = Vec::new();
        }

        if let Err(err) = self.index_workspace().await {
            debug!(
                "Incremental semantic indexing skipped or failed during init: {}",
                err
            );
        }

        Ok(())
    }

    pub fn generate_embedding(&self, text: &str) -> Result<Vec<f32>, String> {
        let model = self.model.as_ref().ok_or("Model not loaded")?;
        let tokenizer = self.tokenizer.as_ref().ok_or("Tokenizer not loaded")?;

        let tokens = tokenizer
            .encode(text, true)
            .map_err(|e| format!("Tokenizer encode failed: {}", e))?;
        let token_ids = tokens.get_ids();
        let token_type_ids = tokens.get_type_ids();

        let input_ids = Tensor::new(token_ids, &self.device)
            .map_err(|e| e.to_string())?
            .unsqueeze(0)
            .map_err(|e| e.to_string())?;

        let token_type_ids = Tensor::new(token_type_ids, &self.device)
            .map_err(|e| e.to_string())?
            .unsqueeze(0)
            .map_err(|e| e.to_string())?;

        let output = model
            .forward(&input_ids, &token_type_ids)
            .map_err(|e| e.to_string())?;

        let mean = output.mean(1).map_err(|e| e.to_string())?;
        let vec = mean
            .get(0)
            .map_err(|e| e.to_string())?
            .to_vec1::<f32>()
            .map_err(|e| e.to_string())?;

        if vec.len() != INDEX_EMBEDDING_DIM {
            return Err(format!(
                "Unexpected embedding dimension {} (expected {})",
                vec.len(),
                INDEX_EMBEDDING_DIM
            ));
        }

        Ok(vec)
    }

    pub async fn index_workspace(&self) -> Result<(), String> {
        info!("Indexing workspace incrementally: {:?}", self.workspace);

        let mut files = Vec::new();
        self.collect_files(&self.workspace, &mut files)?;
        files.sort();

        let existing_cache = self.build_existing_file_cache()?;
        let existing_paths: std::collections::HashSet<String> =
            existing_cache.keys().cloned().collect();
        let mut current_paths = std::collections::HashSet::new();

        let mut reused_files = 0usize;
        let mut changed_files = 0usize;
        let mut new_files = 0usize;
        let mut reused_chunks = 0usize;
        let mut regenerated_chunks = 0usize;

        let mut metadata_acc = Vec::new();
        let mut embeddings_acc = Vec::new();

        for file in files {
            let content = std::fs::read_to_string(&file)
                .map_err(|e| format!("Read failed for {:?}: {}", file, e))?;

            let rel_path = file
                .strip_prefix(&self.workspace)
                .unwrap_or(&file)
                .to_string_lossy()
                .to_string();
            current_paths.insert(rel_path.clone());

            if let Some(cached_entries) = existing_cache.get(&rel_path) {
                if Self::is_file_unchanged(&content, cached_entries) {
                    reused_files += 1;
                    reused_chunks += cached_entries.len();
                    for entry in cached_entries {
                        metadata_acc.push(entry.metadata.clone());
                        embeddings_acc.push(entry.embedding.clone());
                    }
                    continue;
                }
                changed_files += 1;
            } else {
                new_files += 1;
            }

            let chunks = self.chunk_text(&content, CHUNK_LINES);

            for (i, chunk) in chunks.into_iter().enumerate() {
                let embedding = self.generate_embedding(&chunk)?;

                let meta = ChunkMetadata {
                    path: rel_path.clone(),
                    start_line: i * CHUNK_LINES + 1,
                    end_line: (i + 1) * CHUNK_LINES,
                    content: chunk,
                };

                metadata_acc.push(meta);
                embeddings_acc.push(embedding);
                regenerated_chunks += 1;
            }
        }

        let hnsw = Hnsw::new(
            HNSW_MAX_CONNECTIONS,
            HNSW_MAX_ELEMENTS,
            HNSW_MAX_LAYERS,
            HNSW_EF_CONSTRUCTION,
            DistCosine {},
        );
        for (id, embedding) in embeddings_acc.iter().enumerate() {
            hnsw.parallel_insert(&[(embedding, id)]);
        }

        *self.index_store.write().map_err(|e| e.to_string())? = Some(hnsw);
        *self.metadata.write().map_err(|e| e.to_string())? = metadata_acc;
        *self.embeddings.write().map_err(|e| e.to_string())? = embeddings_acc;

        let deleted_files = existing_paths.difference(&current_paths).count();
        info!(
            reused_files = reused_files,
            changed_files = changed_files,
            new_files = new_files,
            deleted_files = deleted_files,
            reused_chunks = reused_chunks,
            regenerated_chunks = regenerated_chunks,
            "Incremental semantic indexing completed"
        );

        info!("Indexing complete. Saving to disk.");
        self.save().await
    }

    pub async fn save(&self) -> Result<(), String> {
        let index_dir = self.workspace.join(".ownstack").join("index");
        std::fs::create_dir_all(&index_dir)
            .map_err(|e| format!("Failed to create index dir: {}", e))?;

        let metadata = self.metadata.read().map_err(|e| e.to_string())?.clone();
        let embeddings = self.embeddings.read().map_err(|e| e.to_string())?.clone();

        if metadata.len() != embeddings.len() {
            return Err(format!(
                "Inconsistent persistence state: metadata={} embeddings={}",
                metadata.len(),
                embeddings.len()
            ));
        }

        for (idx, emb) in embeddings.iter().enumerate() {
            if emb.len() != INDEX_EMBEDDING_DIM {
                return Err(format!(
                    "Invalid embedding dim at index {}: {}",
                    idx,
                    emb.len()
                ));
            }
        }

        let payload = PersistedIndexData {
            embeddings,
            metadata,
        };
        let payload_bytes = serde_json::to_vec(&payload)
            .map_err(|e| format!("Failed to serialize index payload: {}", e))?;
        let checksum = Self::checksum_hex(&payload_bytes);

        let manifest = IndexManifest {
            schema_version: INDEX_SCHEMA_VERSION,
            model_id: INDEX_MODEL_ID.to_string(),
            embedding_dim: INDEX_EMBEDDING_DIM,
            entry_count: payload.metadata.len(),
            checksum_sha256: checksum,
        };

        let manifest_path = index_dir.join("manifest.json");
        let data_path = index_dir.join("index_data.json");

        std::fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest)
                .map_err(|e| format!("Failed to serialize manifest: {}", e))?,
        )
        .map_err(|e| format!("Failed to write manifest: {}", e))?;

        std::fs::write(&data_path, payload_bytes)
            .map_err(|e| format!("Failed to write payload: {}", e))?;

        Ok(())
    }

    pub async fn load(&self) -> Result<(), String> {
        let index_dir = self.workspace.join(".ownstack").join("index");
        let manifest_path = index_dir.join("manifest.json");
        let data_path = index_dir.join("index_data.json");

        if !manifest_path.exists() || !data_path.exists() {
            return Err("Index files not found".to_string());
        }

        let manifest_raw = std::fs::read_to_string(&manifest_path)
            .map_err(|e| format!("Failed to read manifest: {}", e))?;
        let manifest: IndexManifest = serde_json::from_str(&manifest_raw)
            .map_err(|e| format!("Failed to parse manifest: {}", e))?;

        if manifest.schema_version != INDEX_SCHEMA_VERSION {
            return Err(format!(
                "Unsupported index schema version {} (expected {})",
                manifest.schema_version, INDEX_SCHEMA_VERSION
            ));
        }

        if manifest.embedding_dim != INDEX_EMBEDDING_DIM {
            return Err(format!(
                "Unsupported embedding dim {} (expected {})",
                manifest.embedding_dim, INDEX_EMBEDDING_DIM
            ));
        }

        if manifest.model_id != INDEX_MODEL_ID {
            return Err(format!(
                "Model mismatch in index manifest: {}",
                manifest.model_id
            ));
        }

        let payload_bytes = std::fs::read(&data_path)
            .map_err(|e| format!("Failed to read payload: {}", e))?;

        let actual_checksum = Self::checksum_hex(&payload_bytes);
        if actual_checksum != manifest.checksum_sha256 {
            return Err(
                "Index checksum mismatch; persisted index is corrupted".to_string()
            );
        }

        let payload: PersistedIndexData = serde_json::from_slice(&payload_bytes)
            .map_err(|e| format!("Failed to parse payload: {}", e))?;

        if payload.metadata.len() != payload.embeddings.len() {
            return Err(
                "Corrupted payload: metadata/embedding length mismatch".to_string()
            );
        }

        if payload.metadata.len() != manifest.entry_count {
            return Err(format!(
                "Manifest entry_count mismatch: manifest={} payload={}",
                manifest.entry_count,
                payload.metadata.len()
            ));
        }

        for (idx, emb) in payload.embeddings.iter().enumerate() {
            if emb.len() != INDEX_EMBEDDING_DIM {
                return Err(format!(
                    "Corrupted payload embedding dim at {}: {}",
                    idx,
                    emb.len()
                ));
            }
        }

        let hnsw = Hnsw::new(
            HNSW_MAX_CONNECTIONS,
            HNSW_MAX_ELEMENTS,
            HNSW_MAX_LAYERS,
            HNSW_EF_CONSTRUCTION,
            DistCosine {},
        );
        for (idx, emb) in payload.embeddings.iter().enumerate() {
            hnsw.parallel_insert(&[(emb, idx)]);
        }

        *self.index_store.write().map_err(|e| e.to_string())? = Some(hnsw);
        *self.metadata.write().map_err(|e| e.to_string())? = payload.metadata;
        *self.embeddings.write().map_err(|e| e.to_string())? = payload.embeddings;

        Ok(())
    }

    pub async fn search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ChunkMetadata>, String> {
        if self.model.is_none() {
            return Err("Index not initialized. Call init() first.".to_string());
        }

        let query_embedding = self.generate_embedding(query)?;

        let hnsw_lock = self.index_store.read().map_err(|e| e.to_string())?;
        let hnsw = hnsw_lock.as_ref().ok_or("Index store not initialized")?;

        let results = hnsw.search(&query_embedding, limit, 16);
        let meta_lock = self.metadata.read().map_err(|e| e.to_string())?;

        let mut found = Vec::new();
        for res in results {
            if let Some(meta) = meta_lock.get(res.d_id) {
                found.push(meta.clone());
            }
        }

        Ok(found)
    }

    fn collect_files(
        &self,
        dir: &Path,
        files: &mut Vec<PathBuf>,
    ) -> Result<(), String> {
        if !dir.is_dir() {
            return Ok(());
        }

        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if name == ".git" || name == "target" || name == "node_modules" {
                    continue;
                }
                self.collect_files(&path, files)?;
            } else if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                if matches!(ext, "rs" | "py" | "ts" | "js" | "toml" | "md") {
                    files.push(path);
                }
            }
        }
        Ok(())
    }

    fn chunk_text(&self, text: &str, lines_per_chunk: usize) -> Vec<String> {
        let lines: Vec<&str> = text.lines().collect();
        lines
            .chunks(lines_per_chunk)
            .map(|chunk| chunk.join("\n"))
            .collect()
    }

    fn normalize_for_chunking(text: &str) -> String {
        text.lines().collect::<Vec<_>>().join("\n")
    }

    fn build_existing_file_cache(
        &self,
    ) -> Result<HashMap<String, Vec<CachedChunkEntry>>, String> {
        let metadata = self.metadata.read().map_err(|e| e.to_string())?.clone();
        let embeddings = self.embeddings.read().map_err(|e| e.to_string())?.clone();

        let pair_count = metadata.len().min(embeddings.len());
        if metadata.len() != embeddings.len() {
            debug!(
                metadata_len = metadata.len(),
                embeddings_len = embeddings.len(),
                "Metadata/embedding length mismatch; using overlapping entries for incremental cache"
            );
        }

        let mut cache: HashMap<String, Vec<CachedChunkEntry>> = HashMap::new();
        for idx in 0..pair_count {
            cache.entry(metadata[idx].path.clone()).or_default().push(
                CachedChunkEntry {
                    metadata: metadata[idx].clone(),
                    embedding: embeddings[idx].clone(),
                },
            );
        }

        for entries in cache.values_mut() {
            entries.sort_by_key(|entry| entry.metadata.start_line);
        }

        Ok(cache)
    }

    fn is_file_unchanged(
        content: &str,
        cached_entries: &[CachedChunkEntry],
    ) -> bool {
        if cached_entries.is_empty() {
            return Self::normalize_for_chunking(content).is_empty();
        }

        let mut cached_chunks = cached_entries.to_vec();
        cached_chunks.sort_by_key(|entry| entry.metadata.start_line);
        let cached_content = cached_chunks
            .iter()
            .map(|entry| entry.metadata.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        Self::normalize_for_chunking(content) == cached_content
    }

    fn checksum_hex(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let digest = hasher.finalize();
        digest.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample_embedding(seed: f32) -> Vec<f32> {
        vec![seed; INDEX_EMBEDDING_DIM]
    }

    #[tokio::test]
    async fn test_save_load_roundtrip_without_model() {
        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let index = SemanticIndex::new(workspace.clone());

        let emb1 = sample_embedding(0.1);
        let emb2 = sample_embedding(0.2);

        let hnsw = Hnsw::new(
            HNSW_MAX_CONNECTIONS,
            HNSW_MAX_ELEMENTS,
            HNSW_MAX_LAYERS,
            HNSW_EF_CONSTRUCTION,
            DistCosine {},
        );
        hnsw.parallel_insert(&[(&emb1, 0), (&emb2, 1)]);

        *index.index_store.write().unwrap() = Some(hnsw);
        *index.metadata.write().unwrap() = vec![
            ChunkMetadata {
                path: "src/a.rs".to_string(),
                start_line: 1,
                end_line: 20,
                content: "fn a() {}".to_string(),
            },
            ChunkMetadata {
                path: "src/b.rs".to_string(),
                start_line: 21,
                end_line: 40,
                content: "fn b() {}".to_string(),
            },
        ];
        *index.embeddings.write().unwrap() = vec![emb1, emb2];

        index.save().await.unwrap();

        let reloaded = SemanticIndex::new(workspace);
        reloaded.load().await.unwrap();

        assert_eq!(reloaded.metadata.read().unwrap().len(), 2);
        assert_eq!(reloaded.embeddings.read().unwrap().len(), 2);
        assert!(reloaded.index_store.read().unwrap().is_some());
    }

    #[tokio::test]
    async fn test_load_fails_on_checksum_mismatch() {
        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let index = SemanticIndex::new(workspace.clone());

        *index.index_store.write().unwrap() = Some(Hnsw::new(
            HNSW_MAX_CONNECTIONS,
            HNSW_MAX_ELEMENTS,
            HNSW_MAX_LAYERS,
            HNSW_EF_CONSTRUCTION,
            DistCosine {},
        ));
        *index.metadata.write().unwrap() = vec![ChunkMetadata {
            path: "src/a.rs".to_string(),
            start_line: 1,
            end_line: 20,
            content: "fn a() {}".to_string(),
        }];
        *index.embeddings.write().unwrap() = vec![sample_embedding(0.3)];
        index.save().await.unwrap();

        let data_path = workspace
            .join(".ownstack")
            .join("index")
            .join("index_data.json");
        std::fs::write(&data_path, b"corrupted").unwrap();

        let reloaded = SemanticIndex::new(workspace);
        let err = reloaded.load().await.unwrap_err();
        assert!(
            err.contains("checksum mismatch")
                || err.contains("Failed to parse payload")
        );
    }

    #[tokio::test]
    async fn test_load_fails_on_schema_mismatch() {
        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let index = SemanticIndex::new(workspace.clone());

        *index.index_store.write().unwrap() = Some(Hnsw::new(
            HNSW_MAX_CONNECTIONS,
            HNSW_MAX_ELEMENTS,
            HNSW_MAX_LAYERS,
            HNSW_EF_CONSTRUCTION,
            DistCosine {},
        ));
        *index.metadata.write().unwrap() = vec![ChunkMetadata {
            path: "src/a.rs".to_string(),
            start_line: 1,
            end_line: 20,
            content: "fn a() {}".to_string(),
        }];
        *index.embeddings.write().unwrap() = vec![sample_embedding(0.4)];
        index.save().await.unwrap();

        let manifest_path = workspace
            .join(".ownstack")
            .join("index")
            .join("manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap())
                .unwrap();
        manifest["schema_version"] = serde_json::json!(INDEX_SCHEMA_VERSION + 1);
        std::fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let reloaded = SemanticIndex::new(workspace);
        let err = reloaded.load().await.unwrap_err();
        assert!(err.contains("Unsupported index schema version"));
    }

    #[test]
    fn test_normalize_for_chunking_ignores_trailing_newline() {
        let normalized = SemanticIndex::normalize_for_chunking("a\nb\n");
        assert_eq!(normalized, "a\nb");
    }

    #[test]
    fn test_is_file_unchanged_true_for_equivalent_chunked_content() {
        let cached_entries = vec![
            CachedChunkEntry {
                metadata: ChunkMetadata {
                    path: "src/main.rs".to_string(),
                    start_line: 1,
                    end_line: 20,
                    content: "line1\nline2".to_string(),
                },
                embedding: sample_embedding(0.1),
            },
            CachedChunkEntry {
                metadata: ChunkMetadata {
                    path: "src/main.rs".to_string(),
                    start_line: 21,
                    end_line: 40,
                    content: "line3".to_string(),
                },
                embedding: sample_embedding(0.2),
            },
        ];

        assert!(SemanticIndex::is_file_unchanged(
            "line1\nline2\nline3\n",
            &cached_entries
        ));
    }

    #[test]
    fn test_is_file_unchanged_false_when_file_changes() {
        let cached_entries = vec![CachedChunkEntry {
            metadata: ChunkMetadata {
                path: "src/main.rs".to_string(),
                start_line: 1,
                end_line: 20,
                content: "line1\nline2".to_string(),
            },
            embedding: sample_embedding(0.3),
        }];

        assert!(!SemanticIndex::is_file_unchanged(
            "line1\nline2\nline3",
            &cached_entries
        ));
    }
}
