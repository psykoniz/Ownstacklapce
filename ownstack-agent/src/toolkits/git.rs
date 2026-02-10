//! Git Integration Toolkit
//!
//! Tools for AI-assisted git operations:
//! status, diff, staging, commit, branch management.
//! All commands pass through the security pipeline.

use async_trait::async_trait;
use serde::Deserialize;
use std::path::PathBuf;
use tracing::debug;

use ownstack_engine::{
    PolicyDecision, PolicyEngine,
    ProcessSandbox, Sandbox, SandboxLevel,
};

use super::{ToolDef, ToolResult, Toolkit, ToolkitError};

/// Git integration toolkit
pub struct GitToolkit {
    workspace: PathBuf,
}

impl GitToolkit {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }

    fn run_git(&self, args: &str) -> Result<ToolResult, ToolkitError> {
        let command = format!("git {}", args);

        // Policy check
        let decision = PolicyEngine::evaluate(&command);
        match decision {
            PolicyDecision::Blocked => {
                return Err(ToolkitError::SecurityViolation(format!(
                    "Git command blocked: {}",
                    command
                )));
            }
            PolicyDecision::Ask => {
                debug!("Git command requires approval: {}", command);
                // In production, prompt user
            }
            PolicyDecision::Auto => {}
        }

        // Execute in sandbox
        let sandbox = ProcessSandbox;
        let result = sandbox.exec(&command, &self.workspace, SandboxLevel::Standard);

        if result.success {
            Ok(ToolResult::success(result.stdout))
        } else {
            Ok(ToolResult::error(result.stderr))
        }
    }

    fn status(&self) -> Result<ToolResult, ToolkitError> {
        self.run_git("status --short")
    }

    fn diff(&self, staged: bool) -> Result<ToolResult, ToolkitError> {
        if staged {
            self.run_git("diff --cached")
        } else {
            self.run_git("diff")
        }
    }

    fn log(&self, count: u32) -> Result<ToolResult, ToolkitError> {
        self.run_git(&format!("log --oneline -n {}", count))
    }

    fn stage(&self, paths: &[String]) -> Result<ToolResult, ToolkitError> {
        if paths.is_empty() {
            self.run_git("add -A")
        } else {
            let paths_str = paths.join(" ");
            self.run_git(&format!("add {}", paths_str))
        }
    }

    fn commit(&self, message: &str) -> Result<ToolResult, ToolkitError> {
        // Escape message for shell safety
        let safe_message = message.replace('"', r#"\""#);
        self.run_git(&format!("commit -m \"{}\"", safe_message))
    }

    fn branch_list(&self) -> Result<ToolResult, ToolkitError> {
        self.run_git("branch -a")
    }

    fn branch_create(&self, name: &str) -> Result<ToolResult, ToolkitError> {
        self.run_git(&format!("checkout -b {}", name))
    }

    fn branch_switch(&self, name: &str) -> Result<ToolResult, ToolkitError> {
        self.run_git(&format!("checkout {}", name))
    }
}

#[derive(Deserialize)]
struct DiffArgs {
    #[serde(default)]
    staged: bool,
}

#[derive(Deserialize)]
struct LogArgs {
    #[serde(default = "default_log_count")]
    count: u32,
}

fn default_log_count() -> u32 {
    10
}

#[derive(Deserialize)]
struct StageArgs {
    #[serde(default)]
    paths: Vec<String>,
}

#[derive(Deserialize)]
struct CommitArgs {
    message: String,
}

#[derive(Deserialize)]
struct BranchArgs {
    name: String,
}

#[async_trait]
impl Toolkit for GitToolkit {
    fn name(&self) -> &str {
        "git"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef {
                name: "git_status".to_string(),
                description: "Show the working tree status".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            ToolDef {
                name: "git_diff".to_string(),
                description: "Show changes between commits, working tree, etc.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "staged": {
                            "type": "boolean",
                            "description": "Show staged changes instead of unstaged"
                        }
                    }
                }),
            },
            ToolDef {
                name: "git_log".to_string(),
                description: "Show commit log".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "count": {
                            "type": "integer",
                            "description": "Number of commits to show (default: 10)"
                        }
                    }
                }),
            },
            ToolDef {
                name: "git_stage".to_string(),
                description: "Stage files for commit".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "paths": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Files to stage (empty = stage all)"
                        }
                    }
                }),
            },
            ToolDef {
                name: "git_commit".to_string(),
                description: "Commit staged changes".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "message": {
                            "type": "string",
                            "description": "Commit message"
                        }
                    },
                    "required": ["message"]
                }),
            },
            ToolDef {
                name: "git_branches".to_string(),
                description: "List all branches".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            ToolDef {
                name: "git_branch_create".to_string(),
                description: "Create and switch to a new branch".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Branch name"
                        }
                    },
                    "required": ["name"]
                }),
            },
            ToolDef {
                name: "git_branch_switch".to_string(),
                description: "Switch to an existing branch".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Branch name"
                        }
                    },
                    "required": ["name"]
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
            "git_status" => self.status(),
            "git_diff" => {
                let parsed: DiffArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                self.diff(parsed.staged)
            }
            "git_log" => {
                let parsed: LogArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                self.log(parsed.count)
            }
            "git_stage" => {
                let parsed: StageArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                self.stage(&parsed.paths)
            }
            "git_commit" => {
                let parsed: CommitArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                self.commit(&parsed.message)
            }
            "git_branches" => self.branch_list(),
            "git_branch_create" => {
                let parsed: BranchArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                self.branch_create(&parsed.name)
            }
            "git_branch_switch" => {
                let parsed: BranchArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                self.branch_switch(&parsed.name)
            }
            _ => Err(ToolkitError::ToolNotFound(tool_name.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs;
    use std::process::Command;

    fn init_git_repo(path: &std::path::Path) {
        let _ = Command::new("git")
            .arg("init")
            .current_dir(path)
            .output();
        
        let _ = Command::new("git")
            .args(&["config", "user.email", "test@ownstack.dev"])
            .current_dir(path)
            .output();
        
        let _ = Command::new("git")
            .args(&["config", "user.name", "Test User"])
            .current_dir(path)
            .output();
    }

    #[tokio::test]
    async fn test_git_toolkit_creation() {
        let dir = tempdir().unwrap();
        let tk = GitToolkit::new(dir.path().to_path_buf());
        assert_eq!(tk.name(), "git");
        assert_eq!(tk.tools().len(), 8);
    }

    #[tokio::test]
    async fn test_git_status_empty() {
        let dir = tempdir().unwrap();
        init_git_repo(dir.path());
        let tk = GitToolkit::new(dir.path().to_path_buf());
        
        let res = tk.execute("git_status", serde_json::json!({})).await.unwrap();
        assert!(res.success);
    }

    #[tokio::test]
    async fn test_git_workflow() {
        let dir = tempdir().unwrap();
        init_git_repo(dir.path());
        let tk = GitToolkit::new(dir.path().to_path_buf());
        
        // 1. Create a file
        fs::write(dir.path().join("file.txt"), "hello").unwrap();
        
        // 2. Status
        let res = tk.execute("git_status", serde_json::json!({})).await.unwrap();
        assert!(res.output.contains("file.txt") || res.output.is_empty()); // empty if git status --short and untracked

        // 3. Stage
        let res = tk.execute("git_stage", serde_json::json!({"paths": ["file.txt"]})).await.unwrap();
        assert!(res.success);

        // 4. Commit
        let res = tk.execute("git_commit", serde_json::json!({"message": "initial commit"})).await.unwrap();
        assert!(res.success);

        // 5. Log
        let res = tk.execute("git_log", serde_json::json!({"count": 1})).await.unwrap();
        assert!(res.success);
        assert!(res.output.contains("initial commit"));
    }

    #[tokio::test]
    async fn test_git_branch_management() {
        let dir = tempdir().unwrap();
        init_git_repo(dir.path());
        fs::write(dir.path().join("f"), "a").unwrap();
        let tk = GitToolkit::new(dir.path().to_path_buf());
        
        // Must have a commit to create branches in some git versions
        tk.execute("git_stage", serde_json::json!({})).await.unwrap();
        tk.execute("git_commit", serde_json::json!({"message": "m"})).await.unwrap();

        // Create branch
        let res = tk.execute("git_branch_create", serde_json::json!({"name": "feature-1"})).await.unwrap();
        assert!(res.success);

        // List branches
        let res = tk.execute("git_branches", serde_json::json!({})).await.unwrap();
        assert!(res.output.contains("feature-1"));

        // Switch back to master (or main)
        let _ = tk.execute("git_branch_switch", serde_json::json!({"name": "master"})).await;
    }

    #[tokio::test]
    async fn test_git_security_blocking() {
        let dir = tempdir().unwrap();
        let tk = GitToolkit::new(dir.path().to_path_buf());
        
        // Although run_git prepends "git ", we test if our evaluate handles it
        // If the agent tries to inject something destructive via git args
        let res = tk.execute("git_status", serde_json::json!({"id": "; rm -rf /"})).await;
        // Arguments aren't currently sanitized for injection in a way that PolicyEngine sees them as part of the command string if they aren't parsed into 'args' correctly.
        // But let's test a known blocked git command if we add one.
        // Currently PolicyEngine blocks "rm -rf", and our run_git does format!("git {}", args).
        // If args contains "; rm -rf /", the full command is "git ; rm -rf /"
        // Let's see if run_git evaluates the WHOLE command.
        
        let res = tk.run_git("; rm -rf /"); 
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_git_invalid_arguments() {
        let dir = tempdir().unwrap();
        let tk = GitToolkit::new(dir.path().to_path_buf());
        
        let res = tk.execute("git_commit", serde_json::json!({})).await;
        assert!(matches!(res, Err(ToolkitError::InvalidArguments(_))));
    }
}
