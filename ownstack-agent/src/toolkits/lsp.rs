//! LSP Toolkit
//!
//! Tools for interacting with Language Server Protocol features:
//! diagnostics, goto definition, find references, etc.

use async_trait::async_trait;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use url::Url;

use crate::lsp::LspClient;
use super::{ToolDef, ToolResult, Toolkit, ToolkitError};

/// LSP toolkit for code intelligence operations
pub struct LspToolkit {
    workspace: PathBuf,
    client: Arc<Mutex<Option<Arc<LspClient>>>>,
}

impl LspToolkit {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            workspace,
            client: Arc::new(Mutex::new(None)),
        }
    }

    async fn auto_connect(&self) -> Result<ToolResult, ToolkitError> {
        let mut command = None;
        let mut args = Vec::new();

        // Check for Rust
        if self.workspace.join("Cargo.toml").exists() {
            command = Some("rust-analyzer");
        }
        // Check for Python
        else if self.workspace.join("pyproject.toml").exists() || self.workspace.join("requirements.txt").exists() {
            command = Some("pylsp");
        }
        // Check for Node/TS
        else if self.workspace.join("package.json").exists() {
            command = Some("typescript-language-server");
            args.push("--stdio".to_string());
        }
        // Check for Go
        else if self.workspace.join("go.mod").exists() {
             command = Some("gopls");
        }

        if let Some(cmd) = command {
            self.connect(cmd, args).await
        } else {
            Ok(ToolResult::error("Could not auto-detect language server from workspace files".to_string()))
        }
    }

    async fn connect(&self, command: &str, args: Vec<String>) -> Result<ToolResult, ToolkitError> {
        let client = LspClient::start(command, &args)
            .await
            .map_err(|e| ToolkitError::ExecutionFailed(format!("Failed to start LSP: {}", e)))?;

        // Initialize with workspace root
        let root_uri = Url::from_directory_path(&self.workspace)
            .map_err(|_| ToolkitError::ExecutionFailed("Invalid workspace path".to_string()))?;

        client.initialize(root_uri)
            .await
            .map_err(|e| ToolkitError::ExecutionFailed(format!("LSP initialization failed: {}", e)))?;

        let mut c = self.client.lock().await;
        *c = Some(client);

        Ok(ToolResult::success(format!("Connected to LSP server: {} {:?}", command, args)))
    }

    async fn ensure_client(&self) -> Result<Arc<LspClient>, ToolkitError> {
        let c = self.client.lock().await;
        c.as_ref().cloned().ok_or_else(|| ToolkitError::ExecutionFailed("LSP not connected. Use 'lsp_connect' first.".to_string()))
    }

    fn to_uri(&self, path: &str) -> Result<Url, ToolkitError> {
        let path = Path::new(path);
        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace.join(path)
        };
        
        // Canonicalize to ensure correct URI (resolve symlinks, etc)
        let abs_path = abs_path.canonicalize().unwrap_or(abs_path);

        Url::from_file_path(abs_path)
            .map_err(|_| ToolkitError::InvalidArguments(format!("Invalid path for URL conversion: {}", path.display())))
    }

    async fn get_diagnostics(&self, path: &str) -> Result<ToolResult, ToolkitError> {
        let client = self.ensure_client().await?;
        let uri = self.to_uri(path)?;
        
        let diags = client.get_diagnostics(&uri).await;
        
        match diags {
            Some(d) => Ok(ToolResult::success(serde_json::to_string_pretty(&d).unwrap_or_default())),
            None => Ok(ToolResult::success("No diagnostics available (file not open or no errors)".to_string())),
        }
    }

    async fn goto_definition(&self, path: &str, line: u32, column: u32) -> Result<ToolResult, ToolkitError> {
        let client = self.ensure_client().await?;
        let uri = self.to_uri(path)?;
        
        let result = client.goto_definition(uri, line, column).await
            .map_err(|e| ToolkitError::ExecutionFailed(format!("Request failed: {}", e)))?;
            
        Ok(ToolResult::success(serde_json::to_string_pretty(&result).unwrap_or_default()))
    }

    async fn find_references(&self, path: &str, line: u32, column: u32) -> Result<ToolResult, ToolkitError> {
        let client = self.ensure_client().await?;
        let uri = self.to_uri(path)?;
        
        let result = client.find_references(uri, line, column).await
             .map_err(|e| ToolkitError::ExecutionFailed(format!("Request failed: {}", e)))?;

        Ok(ToolResult::success(serde_json::to_string_pretty(&result).unwrap_or_default()))
    }

    async fn get_symbols(&self, path: &str) -> Result<ToolResult, ToolkitError> {
        let client = self.ensure_client().await?;
        let uri = self.to_uri(path)?;
        
        let result = client.document_symbol(uri).await
             .map_err(|e| ToolkitError::ExecutionFailed(format!("Request failed: {}", e)))?;

        Ok(ToolResult::success(serde_json::to_string_pretty(&result).unwrap_or_default()))
    }

    async fn hover(&self, path: &str, line: u32, column: u32) -> Result<ToolResult, ToolkitError> {
        let client = self.ensure_client().await?;
        let uri = self.to_uri(path)?;

        let result = client.hover(uri, line, column).await
             .map_err(|e| ToolkitError::ExecutionFailed(format!("Request failed: {}", e)))?;

        Ok(ToolResult::success(serde_json::to_string_pretty(&result).unwrap_or_default()))
    }
}

#[derive(Deserialize)]
struct ConnectArgs {
    command: String,
    args: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct PathArgs {
    path: String,
}

#[derive(Deserialize)]
struct LocationArgs {
    path: String,
    line: u32,
    column: u32,
}

#[async_trait]
impl Toolkit for LspToolkit {
    fn name(&self) -> &str {
        "lsp"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef {
                name: "lsp_connect".to_string(),
                description: "Connect to an LSP server".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string" },
                        "args": { "type": "array", "items": { "type": "string" } }
                    },
                    "required": ["command"]
                }),
            },
            ToolDef {
                name: "lsp_auto_connect".to_string(),
                description: "Auto-detect and connect to LSP server based on workspace files".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            ToolDef {
                name: "lsp_diagnostics".to_string(),
                description: "Get diagnostics for a file".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": { "path": { "type": "string" } },
                    "required": ["path"]
                }),
            },
            ToolDef {
                name: "lsp_definition".to_string(),
                description: "Go to definition".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": { 
                        "path": { "type": "string" },
                        "line": { "type": "integer" },
                        "column": { "type": "integer" }
                    },
                    "required": ["path", "line", "column"]
                }),
            },
            ToolDef {
                name: "lsp_references".to_string(),
                description: "Find references".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": { 
                        "path": { "type": "string" },
                        "line": { "type": "integer" },
                        "column": { "type": "integer" }
                    },
                    "required": ["path", "line", "column"]
                }),
            },
            ToolDef {
                name: "lsp_symbols".to_string(),
                description: "Get document symbols".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": { "path": { "type": "string" } },
                    "required": ["path"]
                }),
            },
             ToolDef {
                name: "lsp_hover".to_string(),
                description: "Get hover information".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": { 
                        "path": { "type": "string" },
                        "line": { "type": "integer" },
                        "column": { "type": "integer" }
                    },
                    "required": ["path", "line", "column"]
                }),
            },
        ]
    }

    async fn execute(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<ToolResult, ToolkitError> {
        match tool_name {
            "lsp_connect" => {
                let parsed: ConnectArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                self.connect(&parsed.command, parsed.args.unwrap_or_default()).await
            }
            "lsp_auto_connect" => {
                self.auto_connect().await
            }
            "lsp_diagnostics" => {
                let parsed: PathArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                self.get_diagnostics(&parsed.path).await
            }
            "lsp_definition" => {
                let parsed: LocationArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                self.goto_definition(&parsed.path, parsed.line, parsed.column).await
            }
            "lsp_references" => {
                let parsed: LocationArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                self.find_references(&parsed.path, parsed.line, parsed.column).await
            }
            "lsp_symbols" => {
                let parsed: PathArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                self.get_symbols(&parsed.path).await
            }
            "lsp_hover" => {
                let parsed: LocationArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                self.hover(&parsed.path, parsed.line, parsed.column).await
            }
            _ => Err(ToolkitError::ToolNotFound(tool_name.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lsp_toolkit_name() {
        let toolkit = LspToolkit::new(PathBuf::from("."));
        assert_eq!(toolkit.name(), "lsp");
    }

    #[test]
    fn test_lsp_tools_list() {
        let toolkit = LspToolkit::new(PathBuf::from("."));
        let tools = toolkit.tools();
        assert!(tools.iter().any(|t| t.name == "lsp_connect"));
        assert!(tools.iter().any(|t| t.name == "lsp_diagnostics"));
    }
}
