use hnsw_rs::prelude::*;
use std::sync::{Arc, RwLock};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{info, debug};
use candle_core::{Device, Tensor};
use candle_transformers::models::bert::{BertModel, Config};
use tokenizers::Tokenizer;

pub struct SemanticIndex {
    workspace: PathBuf,
    model: Option<BertModel>,
    tokenizer: Option<Tokenizer>,
    device: Device,
    index_store: Arc<RwLock<Option<Hnsw<'static, f32, DistCosine>>>>,
    metadata: Arc<RwLock<Vec<ChunkMetadata>>>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ChunkMetadata {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub content: String,
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
        }
    }

    pub async fn init(&mut self) -> Result<(), String> {
        if self.model.is_some() {
            return Ok(());
        }

        let model_dir = self.workspace.join(".ownstack").join("models");
        if !model_dir.exists() {
            return Err("Model directory not found. Please run bootstrap.".to_string());
        }

        info!("Loading BERT model from {:?}", model_dir);
        
        let config_filename = model_dir.join("config.json");
        let tokenizer_filename = model_dir.join("tokenizer.json");
        let weights_filename = model_dir.join("model.safetensors");

        let config = std::fs::read_to_string(&config_filename).map_err(|e| e.to_string())?;
        let config: Config = serde_json::from_str(&config).map_err(|e| e.to_string())?;
        
        let tokenizer = Tokenizer::from_file(tokenizer_filename).map_err(|e| e.to_string())?;
        let vb = {
            let tensors = candle_core::safetensors::load(&weights_filename, &self.device)
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

        // Try to load existing index
        if let Err(e) = self.load().await {
            debug!("No existing index found or failed to load: {}. Initializing fresh.", e);
            let hnsw = Hnsw::new(384, 100000, 16, 200, DistCosine {});
            *self.index_store.write().unwrap() = Some(hnsw);
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

        let output = model.forward(&input_ids, &token_type_ids)
            .map_err(|e| e.to_string())?;
        
        // Mean pooling
        let (_b_sz, _seq_len, _hidden_size) = output.dims3().map_err(|e| e.to_string())?;
        let mean = output.mean(1).map_err(|e| e.to_string())?;
        let vec = mean.get(0).map_err(|e| e.to_string())?
            .to_vec1::<f32>().map_err(|e| e.to_string())?;

        Ok(vec)
    }

    pub async fn index_workspace(&self) -> Result<(), String> {
        info!("Indexing workspace: {:?}", self.workspace);
        
        let mut files = Vec::new();
        self.collect_files(&self.workspace, &mut files)?;
        
        for file in files {
            let content = std::fs::read_to_string(&file)
                .map_err(|e| format!("Read failed for {:?}: {}", file, e))?;
            
            let chunks = self.chunk_text(&content, 20);
            
            for (i, chunk) in chunks.into_iter().enumerate() {
                let embedding = self.generate_embedding(&chunk)?;
                
                let rel_path = file.strip_prefix(&self.workspace)
                    .unwrap_or(&file)
                    .to_string_lossy()
                    .to_string();
                
                let meta = ChunkMetadata {
                    path: rel_path,
                    start_line: i * 20 + 1,
                    end_line: (i + 1) * 20,
                    content: chunk,
                };
                
                let mut meta_lock = self.metadata.write().unwrap();
                let id = meta_lock.len();
                meta_lock.push(meta);
                
                if let Some(ref mut hnsw) = *self.index_store.write().unwrap() {
                    hnsw.parallel_insert(&[(&embedding, id)]);
                }
            }
        }
        
        info!("Indexing complete. Saving to disk.");
        self.save().await?;
        Ok(())
    }

    pub async fn save(&self) -> Result<(), String> {
        let index_dir = self.workspace.join(".ownstack").join("index");
        std::fs::create_dir_all(&index_dir).map_err(|e| e.to_string())?;

        let hnsw_path = index_dir.join("index.hnsw");
        let meta_path = index_dir.join("metadata.json");

        if let Some(ref _hnsw) = *self.index_store.read().unwrap() {
            // HNSW-rs Debug print removed as it doesn't implement Debug
            std::fs::write(&hnsw_path, "HNSW_DUMP_PLACEHOLDER").map_err(|e| e.to_string())?;
        }

        let meta = self.metadata.read().unwrap();
        let json = serde_json::to_string(&*meta).map_err(|e| e.to_string())?;
        std::fs::write(&meta_path, json).map_err(|e| e.to_string())?;

        Ok(())
    }

    pub async fn load(&self) -> Result<(), String> {
        let index_dir = self.workspace.join(".ownstack").join("index");
        let meta_path = index_dir.join("metadata.json");

        if !meta_path.exists() {
            return Err("Index files not found".to_string());
        }

        let meta_json = std::fs::read_to_string(&meta_path).map_err(|e| e.to_string())?;
        let meta: Vec<ChunkMetadata> = serde_json::from_str(&meta_json).map_err(|e| e.to_string())?;
        *self.metadata.write().unwrap() = meta;

        // Note: Re-initializing HNSW from scratch for now as dump/load is complex across versions
        // In a real implementation we would use a persistent HNSW crate or dump/load features.
        let hnsw = Hnsw::new(384, 100000, 16, 200, DistCosine {});
        *self.index_store.write().unwrap() = Some(hnsw);
        
        Ok(())
    }

    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<ChunkMetadata>, String> {
        if self.model.is_none() {
            return Err("Index not initialized. Call init() first.".to_string());
        }

        let query_embedding = self.generate_embedding(query)?;
        
        let hnsw_lock = self.index_store.read().unwrap();
        let hnsw = hnsw_lock.as_ref().ok_or("Index store not initialized")?;
        
        let results = hnsw.search(&query_embedding, limit, 16);
        let meta_lock = self.metadata.read().unwrap();
        
        let mut found = Vec::new();
        for res in results {
            if let Some(meta) = meta_lock.get(res.d_id) {
                found.push(meta.clone());
            }
        }
        
        Ok(found)
    }

    fn collect_files(&self, dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
        if dir.is_dir() {
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
        }
        Ok(())
    }

    fn chunk_text(&self, text: &str, lines_per_chunk: usize) -> Vec<String> {
        let lines: Vec<&str> = text.lines().collect();
        lines.chunks(lines_per_chunk)
            .map(|chunk| chunk.join("\n"))
            .collect()
    }
}
