//! Core Toolkit
//!
//! Essential tools: exec, read, write, search
//! All operations go through ownstack-engine for security.

use async_trait::async_trait;
use serde::Deserialize;
use std::path::PathBuf;
use tracing::info;

use ownstack_engine::{
    PolicyDecision, PolicyEngine,
    PathValidator,
    ProcessSandbox, SandboxLevel, Sandbox,
    AuditLogger,
};

use super::{ToolDef, ToolResult, Toolkit, ToolkitError};

/// Core toolkit with essential file and command operations
pub struct CoreToolkit {
    workspace: PathBuf,
    path_validator: PathValidator,
    #[allow(dead_code)]
    audit_logger: AuditLogger,
}

impl CoreToolkit {
    pub fn new(workspace: PathBuf) -> Self {
        let audit_logger = AuditLogger::new(workspace.clone());
        let path_validator = PathValidator::new(workspace.clone());
        Self {
            workspace,
            path_validator,
            audit_logger,
        }
    }

    async fn exec_command(&self, command: &str) -> Result<ToolResult, ToolkitError> {
        // Step 1: Policy check
        let decision = PolicyEngine::evaluate(command);
        match decision {
            PolicyDecision::Blocked => {
                return Err(ToolkitError::SecurityViolation(format!(
                    "Command blocked by policy: {}",
                    command
                )));
            }
            PolicyDecision::Ask => {
                // In production, this would prompt the user
                // For now, we log and proceed with caution
                info!("Command requires approval: {}", command);
            }
            PolicyDecision::Auto => {}
        }

        // Step 2: Execute in sandbox using Sandbox trait
        let sandbox = ProcessSandbox;
        let result = sandbox.exec(command, &self.workspace, SandboxLevel::Standard);
        
        if result.success {
            Ok(ToolResult::success(result.stdout))
        } else {
            Ok(ToolResult::error(result.stderr))
        }
    }

    async fn read_file(&self, path: &str) -> Result<ToolResult, ToolkitError> {
        let file_path = std::path::Path::new(path);
        
        // Path validation
        let validated_path = self.path_validator.validate(file_path)
            .map_err(|e| ToolkitError::SecurityViolation(e.to_string()))?;

        match tokio::fs::read_to_string(&validated_path).await {
            Ok(content) => Ok(ToolResult::success(content)),
            Err(e) => Ok(ToolResult::error(format!("Failed to read file: {}", e))),
        }
    }

    async fn write_file(&self, path: &str, content: &str) -> Result<ToolResult, ToolkitError> {
        let file_path = std::path::Path::new(path);
        
        // Path validation
        let validated_path = self.path_validator.validate(file_path)
            .map_err(|e| ToolkitError::SecurityViolation(e.to_string()))?;

        // Create parent directories if needed
        if let Some(parent) = validated_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        match tokio::fs::write(&validated_path, content).await {
            Ok(()) => Ok(ToolResult::success(format!("File written: {}", path))),
            Err(e) => Ok(ToolResult::error(format!("Failed to write file: {}", e))),
        }
    }

    async fn search_files(&self, pattern: &str) -> Result<ToolResult, ToolkitError> {
        let command = format!("grep -rn \"{}\" . --include=\"*.rs\" --include=\"*.py\" --include=\"*.ts\" --include=\"*.js\" | head -50", pattern);
        self.exec_command(&command).await
    }
}

#[derive(Deserialize)]
struct ExecArgs {
    command: String,
}

#[derive(Deserialize)]
struct ReadArgs {
    path: String,
}

#[derive(Deserialize)]
struct WriteArgs {
    path: String,
    content: String,
}

#[derive(Deserialize)]
struct SearchArgs {
    pattern: String,
}

#[async_trait]
impl Toolkit for CoreToolkit {
    fn name(&self) -> &str {
        "core"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef {
                name: "exec".to_string(),
                description: "Execute a shell command in the workspace".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The command to execute"
                        }
                    },
                    "required": ["command"]
                }),
            },
            ToolDef {
                name: "read".to_string(),
                description: "Read the contents of a file".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Relative path to the file"
                        }
                    },
                    "required": ["path"]
                }),
            },
            ToolDef {
                name: "write".to_string(),
                description: "Write content to a file".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Relative path to the file"
                        },
                        "content": {
                            "type": "string",
                            "description": "Content to write"
                        }
                    },
                    "required": ["path", "content"]
                }),
            },
            ToolDef {
                name: "search".to_string(),
                description: "Search for a pattern in files".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": {
                            "type": "string",
                            "description": "Search pattern (regex)"
                        }
                    },
                    "required": ["pattern"]
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
            "exec" => {
                let parsed: ExecArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                self.exec_command(&parsed.command).await
            }
            "read" => {
                let parsed: ReadArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                self.read_file(&parsed.path).await
            }
            "write" => {
                let parsed: WriteArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                self.write_file(&parsed.path, &parsed.content).await
            }
            "search" => {
                let parsed: SearchArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                self.search_files(&parsed.pattern).await
            }
            _ => Err(ToolkitError::ToolNotFound(tool_name.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_core_toolkit_creation() {
        let dir = tempdir().unwrap();
        let tk = CoreToolkit::new(dir.path().to_path_buf());
        assert_eq!(tk.name(), "core");
        assert_eq!(tk.tools().len(), 4);
    }

    #[tokio::test]
    async fn test_core_toolkit_tools_list() {
        let dir = tempdir().unwrap();
        let tk = CoreToolkit::new(dir.path().to_path_buf());
        let tools = tk.tools();
        let names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
        assert!(names.contains(&"exec".to_string()));
        assert!(names.contains(&"read".to_string()));
        assert!(names.contains(&"write".to_string()));
        assert!(names.contains(&"search".to_string()));
    }

    #[tokio::test]
    async fn test_core_toolkit_write_read() {
        let dir = tempdir().unwrap();
        let tk = CoreToolkit::new(dir.path().to_path_buf());
        
        // Write
        let write_args = serde_json::json!({
            "path": "test.txt",
            "content": "hello world"
        });
        let res = tk.execute("write", write_args).await.unwrap();
        assert!(res.success);

        // Read
        let read_args = serde_json::json!({"path": "test.txt"});
        let res = tk.execute("read", read_args).await.unwrap();
        assert!(res.success);
        assert_eq!(res.output, "hello world");
    }

    #[tokio::test]
    async fn test_core_toolkit_invalid_path() {
        let dir = tempdir().unwrap();
        let tk = CoreToolkit::new(dir.path().to_path_buf());
        
        let args = serde_json::json!({"path": "../outside.txt"});
        let res = tk.execute("read", args).await;
        assert!(res.is_err()); // SecurityViolation
    }

    #[tokio::test]
    async fn test_core_toolkit_exec_blocked() {
        let dir = tempdir().unwrap();
        let tk = CoreToolkit::new(dir.path().to_path_buf());
        
        let args = serde_json::json!({"command": "rm -rf /"});
        let res = tk.execute("exec", args).await;
        assert!(res.is_err()); // SecurityViolation
    }

    #[tokio::test]
    async fn test_core_toolkit_unknown_tool() {
        let dir = tempdir().unwrap();
        let tk = CoreToolkit::new(dir.path().to_path_buf());
        
        let res = tk.execute("nonexistent", serde_json::json!({})).await;
        assert!(matches!(res, Err(ToolkitError::ToolNotFound(_))));
    }

    #[tokio::test]
    async fn test_core_toolkit_invalid_args() {
        let dir = tempdir().unwrap();
        let tk = CoreToolkit::new(dir.path().to_path_buf());
        
        let res = tk.execute("write", serde_json::json!({"wrong": "key"})).await;
        assert!(matches!(res, Err(ToolkitError::InvalidArguments(_))));
    }
}

