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

pub struct SemanticIndex {
    workspace: PathBuf,
    model: Option<BertModel>,
    tokenizer: Option<Tokenizer>,
    device: Device,
    index_store: Arc<RwLock<Option<Hnsw<'static, f32, DistCosine>>>>,
    metadata: Arc<RwLock<Vec<ChunkMetadata>>>,
    embeddings: Arc<RwLock<Vec<Vec<f32>>>>,
    manifest: Arc<RwLock<IndexManifest>>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ChunkMetadata {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub content: String,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
struct FileFingerprint {
    sha256: String,
    size_bytes: u64,
    modified_unix_secs: u64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
struct IndexManifest {
    schema_version: u32,
    files: HashMap<String, FileFingerprint>,
}

impl Default for IndexManifest {
    fn default() -> Self {
        Self {
            schema_version: INDEX_SCHEMA_VERSION,
            files: HashMap::new(),
        }
    }
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
            manifest: Arc::new(RwLock::new(IndexManifest::default())),
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

        let config =
            std::fs::read_to_string(&config_filename).map_err(|e| e.to_string())?;
        let config: Config =
            serde_json::from_str(&config).map_err(|e| e.to_string())?;

        let tokenizer =
            Tokenizer::from_file(tokenizer_filename).map_err(|e| e.to_string())?;
        let vb = {
            let tensors =
                candle_core::safetensors::load(&weights_filename, &self.device)
                    .map_err(|e| e.to_string())?;
            candle_nn::VarBuilder::from_tensors(
                tensors,
                candle_core::DType::F32,
                &self.device,
            )
        };

        let model = BertModel::load(vb, &config).map_err(|e| e.to_string())?;

        self.model = Some(model);
        self.tokenizer = Some(tokenizer);

        if let Err(e) = self.load().await {
            debug!(
                "No existing index found or failed to load: {}. Initializing fresh.",
                e
            );
            let hnsw = Hnsw::new(384, 100000, 16, 200, DistCosine {});
            if let Ok(mut lock) = self.index_store.write() {
                *lock = Some(hnsw);
            }
        }

        Ok(())
    }

    pub fn generate_embedding(&self, text: &str) -> Result<Vec<f32>, String> {
        let model = self.model.as_ref().ok_or("Model not loaded")?;
        let tokenizer = self.tokenizer.as_ref().ok_or("Tokenizer not loaded")?;

        let tokens = tokenizer.encode(text, true).map_err(|e| e.to_string())?;
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

        Ok(vec)
    }

    pub async fn index_workspace(&self) -> Result<(), String> {
        info!("Incremental indexing workspace: {:?}", self.workspace);

        let mut files = Vec::new();
        self.collect_files(&self.workspace, &mut files)?;
        files.sort();

        let old_manifest = self
            .manifest
            .read()
            .map_err(|_| "manifest lock poisoned".to_string())?
            .clone();

        let old_metadata = self
            .metadata
            .read()
            .map_err(|_| "metadata lock poisoned".to_string())?
            .clone();
        let old_embeddings = self
            .embeddings
            .read()
            .map_err(|_| "embeddings lock poisoned".to_string())?
            .clone();

        let mut old_by_path: HashMap<String, Vec<(ChunkMetadata, Vec<f32>)>> =
            HashMap::new();
        for (meta, emb) in old_metadata.into_iter().zip(old_embeddings.into_iter()) {
            old_by_path
                .entry(meta.path.clone())
                .or_default()
                .push((meta, emb));
        }

        let mut new_manifest = IndexManifest::default();
        let mut new_metadata = Vec::new();
        let mut new_embeddings = Vec::new();

        let mut current_fingerprints: HashMap<String, FileFingerprint> =
            HashMap::new();
        for file in &files {
            let rel_path = file
                .strip_prefix(&self.workspace)
                .unwrap_or(file)
                .to_string_lossy()
                .to_string();
            let fp = self.compute_file_fingerprint(file)?;
            current_fingerprints.insert(rel_path, fp);
        }

        let (changed, removed) =
            compute_changed_paths(&old_manifest.files, &current_fingerprints);
        debug!(
            "Semantic index delta: changed={} removed={}",
            changed.len(),
            removed.len()
        );

        for file in files {
            let rel_path = file
                .strip_prefix(&self.workspace)
                .unwrap_or(&file)
                .to_string_lossy()
                .to_string();

            let fingerprint = current_fingerprints
                .get(&rel_path)
                .cloned()
                .ok_or_else(|| "missing fingerprint for file".to_string())?;
            new_manifest
                .files
                .insert(rel_path.clone(), fingerprint.clone());

            let unchanged = old_manifest.files.get(&rel_path) == Some(&fingerprint);
            if unchanged {
                if let Some(existing) = old_by_path.remove(&rel_path) {
                    for (meta, emb) in existing {
                        new_metadata.push(meta);
                        new_embeddings.push(emb);
                    }
                    continue;
                }
            }

            let content = std::fs::read_to_string(&file)
                .map_err(|e| format!("Read failed for {:?}: {}", file, e))?;
            let lines_per_chunk = 20;
            let total_lines = content.lines().count();
            let chunks = self.chunk_text(&content, lines_per_chunk);

            for (i, chunk) in chunks.into_iter().enumerate() {
                let embedding = self.generate_embedding(&chunk)?;
                let meta = ChunkMetadata {
                    path: rel_path.clone(),
                    start_line: i * lines_per_chunk + 1,
                    end_line: ((i + 1) * lines_per_chunk).min(total_lines),
                    content: chunk,
                };
                new_metadata.push(meta);
                new_embeddings.push(embedding);
            }
        }

        {
            let mut meta_lock = self
                .metadata
                .write()
                .map_err(|_| "metadata lock poisoned".to_string())?;
            *meta_lock = new_metadata;
        }
        {
            let mut emb_lock = self
                .embeddings
                .write()
                .map_err(|_| "embeddings lock poisoned".to_string())?;
            *emb_lock = new_embeddings;
        }
        {
            let mut manifest_lock = self
                .manifest
                .write()
                .map_err(|_| "manifest lock poisoned".to_string())?;
            *manifest_lock = new_manifest;
        }

        self.rebuild_hnsw()?;
        self.save().await?;
        info!("Indexing complete with incremental update.");
        Ok(())
    }

    pub async fn save(&self) -> Result<(), String> {
        let index_dir = self.workspace.join(".ownstack").join("index");
        std::fs::create_dir_all(&index_dir).map_err(|e| e.to_string())?;

        let meta_path = index_dir.join("metadata.json");
        let embeddings_path = index_dir.join("embeddings.json");
        let manifest_path = index_dir.join("manifest.json");

        let meta = self
            .metadata
            .read()
            .map_err(|_| "metadata lock poisoned".to_string())?;
        let json = serde_json::to_string(&*meta).map_err(|e| e.to_string())?;
        std::fs::write(&meta_path, json).map_err(|e| e.to_string())?;

        let embs = self
            .embeddings
            .read()
            .map_err(|_| "embeddings lock poisoned".to_string())?;
        let embs_json = serde_json::to_string(&*embs).map_err(|e| e.to_string())?;
        std::fs::write(&embeddings_path, embs_json).map_err(|e| e.to_string())?;

        let manifest = self
            .manifest
            .read()
            .map_err(|_| "manifest lock poisoned".to_string())?;
        let manifest_json =
            serde_json::to_string(&*manifest).map_err(|e| e.to_string())?;
        std::fs::write(&manifest_path, manifest_json).map_err(|e| e.to_string())?;

        info!("Index saved: {} chunks", meta.len());
        Ok(())
    }

    pub async fn load(&self) -> Result<(), String> {
        let index_dir = self.workspace.join(".ownstack").join("index");
        let meta_path = index_dir.join("metadata.json");
        let embeddings_path = index_dir.join("embeddings.json");
        let manifest_path = index_dir.join("manifest.json");

        if !meta_path.exists() {
            return Err("Index files not found".to_string());
        }

        let meta_json =
            std::fs::read_to_string(&meta_path).map_err(|e| e.to_string())?;
        let meta: Vec<ChunkMetadata> =
            serde_json::from_str(&meta_json).map_err(|e| e.to_string())?;
        {
            let mut lock = self
                .metadata
                .write()
                .map_err(|_| "metadata lock poisoned".to_string())?;
            *lock = meta;
        }

        if embeddings_path.exists() {
            let embs_json = std::fs::read_to_string(&embeddings_path)
                .map_err(|e| e.to_string())?;
            let embs: Vec<Vec<f32>> =
                serde_json::from_str(&embs_json).map_err(|e| e.to_string())?;
            let mut lock = self
                .embeddings
                .write()
                .map_err(|_| "embeddings lock poisoned".to_string())?;
            *lock = embs;
        }

        if manifest_path.exists() {
            let manifest_json = std::fs::read_to_string(&manifest_path)
                .map_err(|e| e.to_string())?;
            let manifest: IndexManifest =
                serde_json::from_str(&manifest_json).map_err(|e| e.to_string())?;
            let mut lock = self
                .manifest
                .write()
                .map_err(|_| "manifest lock poisoned".to_string())?;
            *lock = manifest;
        }

        self.rebuild_hnsw()?;
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

        let hnsw_lock = self
            .index_store
            .read()
            .map_err(|_| "index lock poisoned".to_string())?;
        let hnsw = hnsw_lock.as_ref().ok_or("Index store not initialized")?;

        let results = hnsw.search(&query_embedding, limit, 16);
        let meta_lock = self
            .metadata
            .read()
            .map_err(|_| "metadata lock poisoned".to_string())?;

        let mut found = Vec::new();
        for res in results {
            if let Some(meta) = meta_lock.get(res.d_id) {
                found.push(meta.clone());
            }
        }

        Ok(found)
    }

    fn rebuild_hnsw(&self) -> Result<(), String> {
        let hnsw = Hnsw::new(384, 100000, 16, 200, DistCosine {});

        let embs = self
            .embeddings
            .read()
            .map_err(|_| "embeddings lock poisoned".to_string())?;
        let indexed: Vec<(&Vec<f32>, usize)> =
            embs.iter().enumerate().map(|(i, e)| (e, i)).collect();
        if !indexed.is_empty() {
            hnsw.parallel_insert(&indexed);
        }

        let mut lock = self
            .index_store
            .write()
            .map_err(|_| "index lock poisoned".to_string())?;
        *lock = Some(hnsw);
        Ok(())
    }

    fn compute_file_fingerprint(
        &self,
        path: &Path,
    ) -> Result<FileFingerprint, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
        let modified_unix_secs = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Ok(file_fingerprint_from_bytes(
            &bytes,
            metadata.len(),
            modified_unix_secs,
        ))
    }

    fn collect_files(
        &self,
        dir: &Path,
        files: &mut Vec<PathBuf>,
    ) -> Result<(), String> {
        if dir.is_dir() {
            for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let path = entry.path();
                if path.is_dir() {
                    let name =
                        path.file_name().and_then(|s| s.to_str()).unwrap_or("");
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
}

fn file_fingerprint_from_bytes(
    bytes: &[u8],
    size_bytes: u64,
    modified_unix_secs: u64,
) -> FileFingerprint {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let sha256 = format!("{:x}", hasher.finalize());
    FileFingerprint {
        sha256,
        size_bytes,
        modified_unix_secs,
    }
}

fn compute_changed_paths(
    previous: &HashMap<String, FileFingerprint>,
    current: &HashMap<String, FileFingerprint>,
) -> (Vec<String>, Vec<String>) {
    let mut changed = Vec::new();
    let mut removed = Vec::new();

    for (path, fp) in current {
        if previous.get(path) != Some(fp) {
            changed.push(path.clone());
        }
    }

    for path in previous.keys() {
        if !current.contains_key(path) {
            removed.push(path.clone());
        }
    }

    changed.sort();
    removed.sort();
    (changed, removed)
}

#[cfg(test)]
mod tests {
    use super::{
        compute_changed_paths, file_fingerprint_from_bytes, FileFingerprint,
    };
    use std::collections::HashMap;

    #[test]
    fn fingerprint_changes_when_content_changes() {
        let a = file_fingerprint_from_bytes(b"hello", 5, 10);
        let b = file_fingerprint_from_bytes(b"hello!", 6, 10);
        assert_ne!(a, b);
    }

    #[test]
    fn changed_path_detection_reports_changed_and_removed() {
        let mut previous: HashMap<String, FileFingerprint> = HashMap::new();
        previous.insert(
            "src/a.rs".to_string(),
            file_fingerprint_from_bytes(b"a", 1, 10),
        );
        previous.insert(
            "src/b.rs".to_string(),
            file_fingerprint_from_bytes(b"b", 1, 10),
        );

        let mut current: HashMap<String, FileFingerprint> = HashMap::new();
        current.insert(
            "src/a.rs".to_string(),
            file_fingerprint_from_bytes(b"a2", 2, 11),
        );
        current.insert(
            "src/c.rs".to_string(),
            file_fingerprint_from_bytes(b"c", 1, 12),
        );

        let (changed, removed) = compute_changed_paths(&previous, &current);
        assert_eq!(
            changed,
            vec!["src/a.rs".to_string(), "src/c.rs".to_string()]
        );
        assert_eq!(removed, vec!["src/b.rs".to_string()]);
    }
}
