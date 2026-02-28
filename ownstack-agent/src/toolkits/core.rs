//! Core Toolkit
//!
//! Essential tools: exec, read, write, edit, search
//! All operations go through ownstack-engine for security.

use async_trait::async_trait;
use regex::Regex;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tracing::warn;

use ownstack_engine::{
    AuditEntry, AuditLogger, PathValidator, PolicyDecision, PolicyEngine,
    ProcessSandbox, Sandbox, SandboxLevel,
};

use crate::policy_approval::PolicyApprovalManager;

use super::{ToolDef, ToolResult, Toolkit, ToolkitError};

/// Core toolkit with essential file and command operations
pub struct CoreToolkit {
    workspace: PathBuf,
    session_id: String,
    path_validator: PathValidator,
    audit_logger: AuditLogger,
    approval: Option<Arc<PolicyApprovalManager>>,
}

impl CoreToolkit {
    pub fn new(
        workspace: PathBuf,
        session_id: String,
        approval: Option<Arc<PolicyApprovalManager>>,
    ) -> Self {
        let audit_logger = AuditLogger::new(workspace.clone());
        let path_validator = PathValidator::new(workspace.clone());
        Self {
            workspace,
            session_id,
            path_validator,
            audit_logger,
            approval,
        }
    }

    fn audit(
        &self,
        action: &str,
        command: &str,
        policy_decision: PolicyDecision,
        tool_name: &str,
        success: bool,
        duration_ms: u64,
        paths_accessed: Vec<String>,
    ) {
        let entry = AuditEntry {
            timestamp: String::new(),
            session_id: self.session_id.clone(),
            action: action.to_string(),
            command: command.to_string(),
            policy_decision,
            tool_name: tool_name.to_string(),
            success,
            duration_ms,
            workspace: self.workspace.to_string_lossy().to_string(),
            paths_accessed,
        };

        if let Err(err) = self.audit_logger.log(entry) {
            warn!("audit log failed: {}", err);
        }
    }

    async fn exec_command(&self, command: &str) -> Result<ToolResult, ToolkitError> {
        let start = Instant::now();

        // Step 1: Policy check
        let decision = PolicyEngine::evaluate(command);
        match decision {
            PolicyDecision::Blocked => {
                self.audit(
                    "exec",
                    command,
                    PolicyDecision::Blocked,
                    "core.exec",
                    false,
                    start.elapsed().as_millis() as u64,
                    Vec::new(),
                );
                return Err(ToolkitError::SecurityViolation(format!(
                    "Command blocked by policy: {}",
                    command
                )));
            }
            PolicyDecision::Ask => {
                if let Some(approval) = self.approval.as_ref() {
                    let approved = approval
                        .request(
                            command.to_string(),
                            "Command requires user approval".to_string(),
                            None,
                        )
                        .await;
                    if !approved {
                        self.audit(
                            "exec",
                            command,
                            PolicyDecision::Ask,
                            "core.exec",
                            false,
                            start.elapsed().as_millis() as u64,
                            Vec::new(),
                        );
                        return Err(ToolkitError::SecurityViolation(format!(
                            "Command denied by user: {}",
                            command
                        )));
                    }
                } else {
                    self.audit(
                        "exec",
                        command,
                        PolicyDecision::Ask,
                        "core.exec",
                        false,
                        start.elapsed().as_millis() as u64,
                        Vec::new(),
                    );
                    return Err(ToolkitError::SecurityViolation(format!(
                        "Command requires approval but UI is not connected: {}",
                        command
                    )));
                }
            }
            PolicyDecision::Auto => {}
        }

        // Step 2: Execute in sandbox using Sandbox trait
        let sandbox = ProcessSandbox;
        let result = sandbox
            .exec(command, &self.workspace, SandboxLevel::Standard)
            .await;

        self.audit(
            "exec",
            command,
            decision,
            "core.exec",
            result.success,
            start.elapsed().as_millis() as u64,
            Vec::new(),
        );
        Ok(result)
    }

    async fn read_file(&self, path: &str) -> Result<ToolResult, ToolkitError> {
        let start = Instant::now();
        let file_path = std::path::Path::new(path);

        // Path validation
        let validated_path =
            self.path_validator.validate(file_path).map_err(|e| {
                self.audit(
                    "read",
                    path,
                    PolicyDecision::Blocked,
                    "core.read",
                    false,
                    start.elapsed().as_millis() as u64,
                    vec![path.to_string()],
                );
                ToolkitError::SecurityViolation(e.to_string())
            })?;

        let res = match tokio::fs::read_to_string(&validated_path).await {
            Ok(content) => Ok(ToolResult::success(content)),
            Err(e) => Ok(ToolResult::failure(
                format!("Failed to read file: {}", e),
                None,
            )),
        };

        if let Ok(result) = &res {
            self.audit(
                "read",
                path,
                PolicyDecision::Auto,
                "core.read",
                result.success,
                start.elapsed().as_millis() as u64,
                vec![validated_path.to_string_lossy().to_string()],
            );
        }

        res
    }

    async fn write_file(
        &self,
        path: &str,
        content: &str,
    ) -> Result<ToolResult, ToolkitError> {
        let start = Instant::now();
        let file_path = std::path::Path::new(path);

        // Path validation
        let validated_path =
            self.path_validator.validate(file_path).map_err(|e| {
                self.audit(
                    "write",
                    path,
                    PolicyDecision::Blocked,
                    "core.write",
                    false,
                    start.elapsed().as_millis() as u64,
                    vec![path.to_string()],
                );
                ToolkitError::SecurityViolation(e.to_string())
            })?;

        // Create parent directories if needed
        if let Some(parent) = validated_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        let res = match tokio::fs::write(&validated_path, content).await {
            Ok(()) => Ok(ToolResult::success(format!("File written: {}", path))),
            Err(e) => Ok(ToolResult::failure(
                format!("Failed to write file: {}", e),
                None,
            )),
        };

        if let Ok(result) = &res {
            self.audit(
                "write",
                path,
                PolicyDecision::Auto,
                "core.write",
                result.success,
                start.elapsed().as_millis() as u64,
                vec![validated_path.to_string_lossy().to_string()],
            );
        }

        res
    }

    async fn edit_file(
        &self,
        path: &str,
        old_text: &str,
        new_text: &str,
    ) -> Result<ToolResult, ToolkitError> {
        let start = Instant::now();
        let file_path = std::path::Path::new(path);

        // Path validation
        let validated_path =
            self.path_validator.validate(file_path).map_err(|e| {
                self.audit(
                    "edit",
                    path,
                    PolicyDecision::Blocked,
                    "core.edit",
                    false,
                    start.elapsed().as_millis() as u64,
                    vec![path.to_string()],
                );
                ToolkitError::SecurityViolation(e.to_string())
            })?;

        // Read existing content
        let content = match tokio::fs::read_to_string(&validated_path).await {
            Ok(c) => c,
            Err(e) => {
                self.audit(
                    "edit",
                    path,
                    PolicyDecision::Auto,
                    "core.edit",
                    false,
                    start.elapsed().as_millis() as u64,
                    vec![validated_path.to_string_lossy().to_string()],
                );
                return Ok(ToolResult::failure(
                    format!("Failed to read file for editing: {}", e),
                    None,
                ));
            }
        };

        // Find and replace
        let occurrences = content.matches(old_text).count();
        if occurrences == 0 {
            self.audit(
                "edit",
                path,
                PolicyDecision::Auto,
                "core.edit",
                false,
                start.elapsed().as_millis() as u64,
                vec![validated_path.to_string_lossy().to_string()],
            );
            return Ok(ToolResult::failure(
                "old_text not found in file".to_string(),
                None,
            ));
        }

        if occurrences > 1 {
            self.audit(
                "edit",
                path,
                PolicyDecision::Auto,
                "core.edit",
                false,
                start.elapsed().as_millis() as u64,
                vec![validated_path.to_string_lossy().to_string()],
            );
            return Ok(ToolResult::failure(
                format!(
                    "old_text found {} times — must be unique. Provide more context.",
                    occurrences
                ),
                None,
            ));
        }

        let new_content = content.replacen(old_text, new_text, 1);
        let res = match tokio::fs::write(&validated_path, &new_content).await {
            Ok(()) => Ok(ToolResult::success(format!(
                "File edited: {} (1 replacement)",
                path
            ))),
            Err(e) => Ok(ToolResult::failure(
                format!("Failed to write edited file: {}", e),
                None,
            )),
        };

        if let Ok(result) = &res {
            self.audit(
                "edit",
                path,
                PolicyDecision::Auto,
                "core.edit",
                result.success,
                start.elapsed().as_millis() as u64,
                vec![validated_path.to_string_lossy().to_string()],
            );
        }

        res
    }

    async fn search_files(&self, pattern: &str) -> Result<ToolResult, ToolkitError> {
        let start = Instant::now();
        let pattern = pattern.trim();
        if pattern.is_empty() {
            self.audit(
                "search",
                pattern,
                PolicyDecision::Auto,
                "core.search",
                false,
                start.elapsed().as_millis() as u64,
                Vec::new(),
            );
            return Err(ToolkitError::InvalidArguments(
                "pattern must not be empty".to_string(),
            ));
        }

        let regex = Regex::new(pattern).map_err(|err| {
            ToolkitError::InvalidArguments(format!("Invalid regex pattern: {err}"))
        })?;

        let workspace = self.workspace.clone();
        let pattern_for_audit = pattern.to_string();

        // File walking + reading is blocking; keep it off the async runtime.
        let join_res =
            tokio::task::spawn_blocking(move || search_workspace(workspace, regex))
                .await;

        let (tool_result, paths_accessed) = match join_res {
            Ok(Ok((stdout, paths))) => (ToolResult::success(stdout), paths),
            Ok(Err(err)) => (ToolResult::failure(err, None), Vec::new()),
            Err(err) => (
                ToolResult::failure(format!("Search task failed: {err}"), None),
                Vec::new(),
            ),
        };

        self.audit(
            "search",
            &pattern_for_audit,
            PolicyDecision::Auto,
            "core.search",
            tool_result.success,
            start.elapsed().as_millis() as u64,
            paths_accessed,
        );

        Ok(tool_result)
    }
}

fn search_workspace(
    workspace: PathBuf,
    regex: Regex,
) -> Result<(String, Vec<String>), String> {
    const MAX_MATCHES: usize = 200;
    const MAX_FILE_BYTES: u64 = 1_000_000; // avoid huge files
    const MAX_AUDIT_PATHS: usize = 20;

    fn is_allowed_ext(path: &std::path::Path) -> bool {
        let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
            // Allow well-known extensionless config files
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                return false;
            };
            return matches!(
                name,
                "Makefile"
                    | "Dockerfile"
                    | "Jenkinsfile"
                    | "Procfile"
                    | ".gitignore"
                    | ".env.example"
            );
        };
        matches!(
            ext,
            // Systems programming
            "rs" | "go" | "c" | "h" | "cpp" | "hpp" | "cc"
            // Web / scripting
            | "py" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs"
            // JVM
            | "java" | "kt" | "scala" | "groovy"
            // Config / data
            | "toml" | "yaml" | "yml" | "json" | "xml" | "ini" | "cfg"
            // Shell / DevOps
            | "sh" | "bash" | "zsh" | "fish" | "ps1"
            // Web markup / style
            | "html" | "htm" | "css" | "scss" | "less" | "svelte" | "vue"
            // Documentation
            | "md" | "rst" | "txt"
            // Other
            | "sql" | "graphql" | "proto" | "tf" | "hcl"
            | "rb" | "php" | "swift" | "zig" | "nim" | "lua"
            | "ex" | "exs" | "erl" | "hs" | "ml" | "clj"
        )
    }

    fn should_skip_dir(name: &std::ffi::OsStr) -> bool {
        let Some(name) = name.to_str() else {
            return true;
        };
        matches!(
            name,
            ".git"
                | "target"
                | "node_modules"
                | ".ownstack"
                | "artifacts"
                | "dist"
                | "build"
        )
    }

    fn search_dir(
        workspace_root: &std::path::Path,
        dir: &std::path::Path,
        regex: &Regex,
        out: &mut Vec<String>,
        audit_paths: &mut Vec<String>,
    ) -> Result<(), std::io::Error> {
        let mut entries: Vec<std::fs::DirEntry> =
            std::fs::read_dir(dir)?.filter_map(Result::ok).collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            if out.len() >= MAX_MATCHES {
                return Ok(());
            }

            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            let path = entry.path();

            // Avoid leaving the workspace via symlinks/junctions.
            if file_type.is_symlink() {
                continue;
            }

            if file_type.is_dir() {
                if should_skip_dir(&entry.file_name()) {
                    continue;
                }
                let _ = search_dir(workspace_root, &path, regex, out, audit_paths);
                continue;
            }

            if !file_type.is_file() {
                continue;
            }
            if !is_allowed_ext(&path) {
                continue;
            }

            let meta = match std::fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.len() > MAX_FILE_BYTES {
                continue;
            }

            let bytes = match std::fs::read(&path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            // Likely binary; skip.
            if bytes.iter().any(|b| *b == 0) {
                continue;
            }

            let text = String::from_utf8_lossy(&bytes);
            for (idx, line) in text.lines().enumerate() {
                if out.len() >= MAX_MATCHES {
                    break;
                }
                if !regex.is_match(line) {
                    continue;
                }

                let rel = path.strip_prefix(workspace_root).unwrap_or(&path);
                let rel_s = rel.to_string_lossy().to_string();

                // Keep audit payload bounded.
                if audit_paths.len() < MAX_AUDIT_PATHS
                    && !audit_paths.iter().any(|p| p == &rel_s)
                {
                    audit_paths.push(rel_s.clone());
                }

                let mut snippet = line.trim_end().to_string();
                if snippet.len() > 240 {
                    snippet.truncate(240);
                    snippet.push_str("...");
                }
                out.push(format!("{}:{}:{}", rel_s, idx + 1, snippet));
            }
        }

        Ok(())
    }

    let mut results: Vec<String> = Vec::new();
    let mut audit_paths: Vec<String> = Vec::new();

    search_dir(
        &workspace,
        &workspace,
        &regex,
        &mut results,
        &mut audit_paths,
    )
    .map_err(|e| format!("Search failed: {e}"))?;

    if results.is_empty() {
        Ok(("No matches found.".to_string(), audit_paths))
    } else {
        Ok((results.join("\n"), audit_paths))
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

#[derive(Deserialize)]
struct EditArgs {
    path: String,
    old_text: String,
    new_text: String,
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
                name: "edit".to_string(),
                description: "Edit a file by replacing exact text. old_text must appear exactly once.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Relative path to the file"
                        },
                        "old_text": {
                            "type": "string",
                            "description": "Exact text to find (must be unique in file)"
                        },
                        "new_text": {
                            "type": "string",
                            "description": "Replacement text"
                        }
                    },
                    "required": ["path", "old_text", "new_text"]
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
            "edit" => {
                let parsed: EditArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                self.edit_file(&parsed.path, &parsed.old_text, &parsed.new_text)
                    .await
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
        let tk =
            CoreToolkit::new(dir.path().to_path_buf(), "test".to_string(), None);
        assert_eq!(tk.name(), "core");
        assert_eq!(tk.tools().len(), 5);
    }

    #[tokio::test]
    async fn test_core_toolkit_tools_list() {
        let dir = tempdir().unwrap();
        let tk =
            CoreToolkit::new(dir.path().to_path_buf(), "test".to_string(), None);
        let tools = tk.tools();
        let names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
        assert!(names.contains(&"exec".to_string()));
        assert!(names.contains(&"read".to_string()));
        assert!(names.contains(&"write".to_string()));
        assert!(names.contains(&"edit".to_string()));
        assert!(names.contains(&"search".to_string()));
    }

    #[tokio::test]
    async fn test_core_toolkit_write_read() {
        let dir = tempdir().unwrap();
        let tk =
            CoreToolkit::new(dir.path().to_path_buf(), "test".to_string(), None);

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
        assert_eq!(res.stdout, "hello world");
    }

    #[tokio::test]
    async fn test_core_toolkit_invalid_path() {
        let dir = tempdir().unwrap();
        let tk =
            CoreToolkit::new(dir.path().to_path_buf(), "test".to_string(), None);

        let args = serde_json::json!({"path": "../outside.txt"});
        let res = tk.execute("read", args).await;
        assert!(res.is_err()); // SecurityViolation
    }

    #[tokio::test]
    async fn test_core_toolkit_exec_blocked() {
        let dir = tempdir().unwrap();
        let tk =
            CoreToolkit::new(dir.path().to_path_buf(), "test".to_string(), None);

        let args = serde_json::json!({"command": "rm -rf /"});
        let res = tk.execute("exec", args).await;
        assert!(res.is_err()); // SecurityViolation
    }

    #[tokio::test]
    async fn test_core_toolkit_unknown_tool() {
        let dir = tempdir().unwrap();
        let tk =
            CoreToolkit::new(dir.path().to_path_buf(), "test".to_string(), None);

        let res = tk.execute("nonexistent", serde_json::json!({})).await;
        assert!(matches!(res, Err(ToolkitError::ToolNotFound(_))));
    }

    #[tokio::test]
    async fn test_core_toolkit_invalid_args() {
        let dir = tempdir().unwrap();
        let tk =
            CoreToolkit::new(dir.path().to_path_buf(), "test".to_string(), None);

        let res = tk
            .execute("write", serde_json::json!({"wrong": "key"}))
            .await;
        assert!(matches!(res, Err(ToolkitError::InvalidArguments(_))));
    }

    #[tokio::test]
    async fn test_core_toolkit_exec_ask_requires_ui() {
        let dir = tempdir().unwrap();
        let tk =
            CoreToolkit::new(dir.path().to_path_buf(), "test".to_string(), None);

        // Likely classified as Ask by policy (network / push operations).
        let args = serde_json::json!({"command": "git push origin main"});
        let res = tk.execute("exec", args).await;
        assert!(res.is_err()); // SecurityViolation (needs approval)
    }

    #[tokio::test]
    async fn test_core_toolkit_search_finds_match() {
        let dir = tempdir().unwrap();
        let tk =
            CoreToolkit::new(dir.path().to_path_buf(), "test".to_string(), None);

        std::fs::write(
            dir.path().join("needle_ownstack_test.rs"),
            "hello world\nno match here\nhello again\n",
        )
        .unwrap();

        let args = serde_json::json!({"pattern": "hello"});
        let res = tk.execute("search", args).await.unwrap();
        assert!(res.success);
        assert!(
            res.stdout.contains("needle_ownstack_test.rs:1:hello world"),
            "stdout was: {}",
            res.stdout
        );
    }

    #[tokio::test]
    async fn test_core_toolkit_search_rejects_invalid_regex() {
        let dir = tempdir().unwrap();
        let tk =
            CoreToolkit::new(dir.path().to_path_buf(), "test".to_string(), None);

        let args = serde_json::json!({"pattern": "["});
        let res = tk.execute("search", args).await;
        assert!(matches!(res, Err(ToolkitError::InvalidArguments(_))));
    }

    #[tokio::test]
    async fn test_core_toolkit_edit_success() {
        let dir = tempdir().unwrap();
        let tk =
            CoreToolkit::new(dir.path().to_path_buf(), "test".to_string(), None);

        // Create file
        std::fs::write(dir.path().join("edit_test.txt"), "hello world\nfoo bar\n")
            .unwrap();

        // Edit
        let args = serde_json::json!({
            "path": "edit_test.txt",
            "old_text": "foo bar",
            "new_text": "baz qux"
        });
        let res = tk.execute("edit", args).await.unwrap();
        assert!(res.success, "edit failed: {}", res.stderr);

        // Verify
        let content =
            std::fs::read_to_string(dir.path().join("edit_test.txt")).unwrap();
        assert_eq!(content, "hello world\nbaz qux\n");
    }

    #[tokio::test]
    async fn test_core_toolkit_edit_not_found() {
        let dir = tempdir().unwrap();
        let tk =
            CoreToolkit::new(dir.path().to_path_buf(), "test".to_string(), None);

        std::fs::write(dir.path().join("edit_nf.txt"), "hello world").unwrap();

        let args = serde_json::json!({
            "path": "edit_nf.txt",
            "old_text": "does not exist",
            "new_text": "replacement"
        });
        let res = tk.execute("edit", args).await.unwrap();
        assert!(!res.success);
        assert!(res.stderr.contains("not found"));
    }

    #[tokio::test]
    async fn test_core_toolkit_edit_ambiguous() {
        let dir = tempdir().unwrap();
        let tk =
            CoreToolkit::new(dir.path().to_path_buf(), "test".to_string(), None);

        std::fs::write(dir.path().join("edit_dup.txt"), "foo\nfoo\nbar\n").unwrap();

        let args = serde_json::json!({
            "path": "edit_dup.txt",
            "old_text": "foo",
            "new_text": "baz"
        });
        let res = tk.execute("edit", args).await.unwrap();
        assert!(!res.success);
        assert!(res.stderr.contains("2 times"));
    }

    #[tokio::test]
    async fn test_core_toolkit_edit_invalid_path() {
        let dir = tempdir().unwrap();
        let tk =
            CoreToolkit::new(dir.path().to_path_buf(), "test".to_string(), None);

        let args = serde_json::json!({
            "path": "../outside.txt",
            "old_text": "a",
            "new_text": "b"
        });
        let res = tk.execute("edit", args).await;
        assert!(res.is_err()); // SecurityViolation
    }
}
