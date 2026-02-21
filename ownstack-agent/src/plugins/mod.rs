//! WASI Plugin System (Wasmtime 14.0.0 implementation)
//!
//! Loads and executes .wasm toolkits in a restricted environment.

use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tracing::{debug, info};
use wasmtime::*;
use wasmtime_wasi::WasiCtxBuilder;

use crate::toolkits::{ToolDef, ToolResult, Toolkit, ToolkitError};

/// Host environment for a WASI plugin
pub struct WasiPluginHost {
    workspace: PathBuf,
    engine: Engine,
}

impl WasiPluginHost {
    pub fn new(workspace: PathBuf) -> Self {
        let mut config = Config::new();
        config.wasm_backtrace_details(WasmBacktraceDetails::Enable);
        let engine = Engine::new(&config).expect("Failed to create Wasmtime engine");

        Self { workspace, engine }
    }

    /// Load all plugins from a directory
    pub async fn load_all(&self, dir: &Path) -> Vec<Arc<dyn Toolkit>> {
        let mut toolkits = Vec::new();
        let mut skipped = 0usize;
        if let Ok(mut entries) = fs::read_dir(dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("wasm") {
                    match self.load_plugin(&path).await {
                        Ok(tk) => toolkits.push(tk),
                        Err(e) => {
                            skipped += 1;
                            // Invalid third-party plugins should not fail startup.
                            debug!("Skipping invalid plugin {:?}: {}", path, e);
                        }
                    }
                }
            }
        }
        if skipped > 0 {
            info!("Skipped {} invalid WASI plugin(s)", skipped);
        }
        toolkits
    }

    /// Load a WASM file and wrap it as a Toolkit
    pub async fn load_plugin(
        &self,
        path: &Path,
    ) -> Result<Arc<dyn Toolkit>, ToolkitError> {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown_plugin")
            .to_string();

        info!("Loading WASI plugin: {} from {:?}", name, path);

        let wasm_bytes = fs::read(path).await.map_err(|e| {
            ToolkitError::ExecutionFailed(format!("Failed to read WASM: {}", e))
        })?;

        let module = Module::new(&self.engine, &wasm_bytes).map_err(|e| {
            ToolkitError::ExecutionFailed(format!("Failed to compile WASM: {}", e))
        })?;

        let toolkit = WasiToolkit::new(
            name,
            module,
            self.engine.clone(),
            self.workspace.clone(),
        );
        Ok(Arc::new(toolkit))
    }
}

/// A toolkit implemented as a WASM module
pub struct WasiToolkit {
    name: String,
    module: Module,
    engine: Engine,
    workspace: PathBuf,
}

impl WasiToolkit {
    pub fn new(
        name: String,
        module: Module,
        engine: Engine,
        workspace: PathBuf,
    ) -> Self {
        Self {
            name,
            module,
            engine,
            workspace,
        }
    }
}

#[async_trait]
impl Toolkit for WasiToolkit {
    fn name(&self) -> &str {
        &self.name
    }

    fn tools(&self) -> Vec<ToolDef> {
        // Simplified: The plugin exports a single tool named after the plugin
        vec![ToolDef {
            name: self.name.clone(),
            description: format!("Plugin toolkit: {}", self.name),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "input": { "type": "string" }
                }
            }),
        }]
    }

    async fn execute(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<ToolResult, ToolkitError> {
        debug!("Executing WASI tool: {} with args: {:?}", tool_name, args);

        // 1. Setup WASI context
        // In Wasmtime 14, we use WasiCtxBuilder
        let wasi = WasiCtxBuilder::new()
            .inherit_stdout()
            .inherit_stderr()
            .preopened_dir(
                wasmtime_wasi::Dir::open_ambient_dir(
                    &self.workspace,
                    wasmtime_wasi::ambient_authority(),
                )
                .map_err(|e| {
                    ToolkitError::SecurityViolation(format!(
                        "Failed to open workspace dir: {}",
                        e
                    ))
                })?,
                "/workspace",
            )
            .map_err(|e| {
                ToolkitError::ExecutionFailed(format!("WASI setup error: {}", e))
            })?
            .build();

        let mut store = Store::new(&self.engine, wasi);

        // 2. Setup Linker
        let mut linker = Linker::new(&self.engine);
        wasmtime_wasi::add_to_linker(&mut linker, |s| s).map_err(|e| {
            ToolkitError::ExecutionFailed(format!("Linker error: {}", e))
        })?;

        // 3. Instantiate
        let instance =
            linker.instantiate(&mut store, &self.module).map_err(|e| {
                ToolkitError::ExecutionFailed(format!("Instantiation error: {}", e))
            })?;

        // 4. Call entry point
        // Look for "run" function
        let func = instance
            .get_typed_func::<(), ()>(&mut store, "run")
            .map_err(|_| {
                ToolkitError::ToolNotFound(format!(
                    "Plugin {} does not export 'run'",
                    self.name
                ))
            })?;

        // Standard ABI: We'll eventually pass JSON via shared memory.
        // For Phase 3 prototype, we just trigger the execution.
        func.call(&mut store, ()).map_err(|e| {
            ToolkitError::ExecutionFailed(format!("WASM execution failed: {}", e))
        })?;

        Ok(ToolResult::success(format!(
            "WASI plugin {} executed successfully",
            self.name
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::WasiPluginHost;
    use tempfile::tempdir;
    use tokio::fs;

    #[tokio::test]
    async fn malformed_wasm_is_rejected() {
        let dir = tempdir().expect("tempdir");
        let wasm_path = dir.path().join("malformed.wasm");
        fs::write(&wasm_path, b"not a wasm binary")
            .await
            .expect("write malformed wasm");

        let host = WasiPluginHost::new(dir.path().to_path_buf());
        let result = host.load_plugin(&wasm_path).await;
        assert!(result.is_err(), "malformed wasm should fail to load");
    }
}
