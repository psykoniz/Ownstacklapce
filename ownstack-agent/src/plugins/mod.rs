//! WASI Plugin System (Wasmtime 14.0.0 implementation)
//!
//! Loads and executes .wasm toolkits in a restricted environment.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tracing::{debug, error, info};
use wasi_common::pipe::{ReadPipe, WritePipe};
use wasmtime::*;
use wasmtime_wasi::WasiCtxBuilder;

use crate::toolkits::{ToolDef, ToolResult, Toolkit, ToolkitError};

const TRUSTED_PLUGIN_PUBKEY_HEX_ENV: &str = "OWNSTACK_PLUGIN_TRUSTED_PUBLIC_KEY_HEX";

fn decode_hex(input: &str) -> Result<Vec<u8>, ToolkitError> {
    let trimmed = input.trim();
    if trimmed.len() % 2 != 0 {
        return Err(ToolkitError::SecurityViolation(
            "trusted public key hex length must be even".to_string(),
        ));
    }

    let mut bytes = Vec::with_capacity(trimmed.len() / 2);
    let chars: Vec<char> = trimmed.chars().collect();
    for i in (0..chars.len()).step_by(2) {
        let hi = chars[i].to_digit(16).ok_or_else(|| {
            ToolkitError::SecurityViolation(format!(
                "invalid hex character '{}' in trusted public key",
                chars[i]
            ))
        })?;
        let lo = chars[i + 1].to_digit(16).ok_or_else(|| {
            ToolkitError::SecurityViolation(format!(
                "invalid hex character '{}' in trusted public key",
                chars[i + 1]
            ))
        })?;
        bytes.push(((hi << 4) | lo) as u8);
    }
    Ok(bytes)
}

fn trusted_public_key_bytes() -> Result<[u8; 32], ToolkitError> {
    match std::env::var(TRUSTED_PLUGIN_PUBKEY_HEX_ENV) {
        Ok(hex) => {
            let decoded = decode_hex(&hex)?;
            decoded.try_into().map_err(|_| {
                ToolkitError::SecurityViolation(format!(
                    "{} must decode to exactly 32 bytes",
                    TRUSTED_PLUGIN_PUBKEY_HEX_ENV
                ))
            })
        }
        Err(_) => Err(ToolkitError::SecurityViolation(format!(
            "{} environment variable is required for plugin signature verification",
            TRUSTED_PLUGIN_PUBKEY_HEX_ENV
        ))),
    }
}

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

        // ─── Phase 12: Secure Marketplace Signature Verification ───
        let sig_path = path.with_extension("wasm.sig");
        if !sig_path.exists() {
            return Err(ToolkitError::SecurityViolation(format!(
                "Missing signature for plugin: {:?}",
                path
            )));
        }

        let signature = fs::read(&sig_path).await.map_err(|e| {
            ToolkitError::SecurityViolation(format!(
                "Failed to read signature: {}",
                e
            ))
        })?;

        // In production this should come from a trusted store.
        // For tests, allow injecting a trusted key through env.
        let trusted_public_key = trusted_public_key_bytes()?;

        let module = Module::new(&self.engine, &wasm_bytes).map_err(|e| {
            ToolkitError::ExecutionFailed(format!("Failed to compile WASM: {}", e))
        })?;

        let toolkit = Arc::new(WasiToolkit::new(
            name,
            module,
            self.engine.clone(),
            self.workspace.clone(),
        ));

        // Use SignedToolkit wrapper to perform verification
        let signed = crate::toolkits::SignedToolkit {
            toolkit: toolkit.clone(),
            signature,
            public_key: trusted_public_key.to_vec(),
        };

        signed.verify().map_err(|e| {
            error!("Signature verification failed for plugin {:?}: {}", path, e);
            e
        })?;

        Ok(toolkit)
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

#[derive(Debug, Serialize)]
struct PluginInput {
    tool_name: String,
    args: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct PluginOutput {
    success: bool,
    output: String,
    #[serde(default)]
    error: Option<String>,
}

fn capture_pipe_output(
    pipe: WritePipe<Cursor<Vec<u8>>>,
    stream_name: &str,
) -> Result<String, ToolkitError> {
    let cursor = pipe.try_into_inner().map_err(|_| {
        ToolkitError::ExecutionFailed(format!(
            "Failed to capture plugin {} (pipe still borrowed)",
            stream_name
        ))
    })?;
    String::from_utf8(cursor.into_inner()).map_err(|e| {
        ToolkitError::ExecutionFailed(format!(
            "Plugin {} is not valid UTF-8: {}",
            stream_name, e
        ))
    })
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
                "description": "Arbitrary JSON arguments forwarded to the plugin.",
                "properties": {},
                "additionalProperties": true
            }),
        }]
    }

    async fn execute(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<ToolResult, ToolkitError> {
        debug!("Executing WASI tool: {} with args: {:?}", tool_name, args);

        if tool_name != self.name {
            return Err(ToolkitError::ToolNotFound(format!(
                "{} (toolkit {})",
                tool_name, self.name
            )));
        }

        let input = PluginInput {
            tool_name: tool_name.to_string(),
            args,
        };
        let input_json = serde_json::to_vec(&input).map_err(|e| {
            ToolkitError::InvalidArguments(format!(
                "Failed to serialize plugin input: {}",
                e
            ))
        })?;

        let stdin_pipe = ReadPipe::from(input_json);
        let stdout_pipe = WritePipe::new_in_memory();
        let stderr_pipe = WritePipe::new_in_memory();

        // 1. Setup WASI context with JSON stdin/stdout ABI.
        let wasi = WasiCtxBuilder::new()
            .stdin(Box::new(stdin_pipe))
            .stdout(Box::new(stdout_pipe.clone()))
            .stderr(Box::new(stderr_pipe.clone()))
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

        // 4. Call entry point.
        // Prefer `run` (our contract), then fall back to `_start` for legacy WASI commands.
        let mut executed = false;

        if let Ok(run_i32) = instance.get_typed_func::<(), i32>(&mut store, "run") {
            let code = run_i32.call(&mut store, ()).map_err(|e| {
                ToolkitError::ExecutionFailed(format!(
                    "WASM execution failed (run -> i32): {}",
                    e
                ))
            })?;
            if code != 0 {
                return Err(ToolkitError::ExecutionFailed(format!(
                    "Plugin {} returned non-zero exit code: {}",
                    self.name, code
                )));
            }
            executed = true;
        }

        if !executed {
            if let Ok(run_unit) =
                instance.get_typed_func::<(), ()>(&mut store, "run")
            {
                run_unit.call(&mut store, ()).map_err(|e| {
                    ToolkitError::ExecutionFailed(format!(
                        "WASM execution failed (run -> ()): {}",
                        e
                    ))
                })?;
                executed = true;
            }
        }

        if !executed {
            if let Ok(start) =
                instance.get_typed_func::<(), ()>(&mut store, "_start")
            {
                start.call(&mut store, ()).map_err(|e| {
                    ToolkitError::ExecutionFailed(format!(
                        "WASM execution failed (_start): {}",
                        e
                    ))
                })?;
                executed = true;
            }
        }

        if !executed {
            return Err(ToolkitError::ToolNotFound(format!(
                "Plugin {} exports neither 'run' nor '_start'",
                self.name
            )));
        }

        // Drop runtime state before collecting pipe buffers.
        drop(linker);
        drop(store);

        let stdout = capture_pipe_output(stdout_pipe, "stdout")?;
        let stderr = capture_pipe_output(stderr_pipe, "stderr")?;

        let output = stdout.trim();
        if output.is_empty() {
            return Err(ToolkitError::ExecutionFailed(format!(
                "Plugin {} returned empty stdout. stderr={}",
                self.name, stderr
            )));
        }

        let parsed: PluginOutput = serde_json::from_str(output).map_err(|e| {
            ToolkitError::ExecutionFailed(format!(
                "Plugin {} returned invalid JSON output: {}. stdout={}",
                self.name, e, output
            ))
        })?;

        if parsed.success {
            Ok(ToolResult::success(parsed.output))
        } else {
            let mut message = parsed.error.unwrap_or(parsed.output);
            if message.trim().is_empty() && !stderr.trim().is_empty() {
                message = stderr;
            }
            Ok(ToolResult::failure(message, None))
        }
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
