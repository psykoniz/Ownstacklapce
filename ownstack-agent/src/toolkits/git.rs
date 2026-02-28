use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info, warn};

use crate::provider::LlmMessage;
use crate::provider::LlmProvider;

use crate::policy_approval::PolicyApprovalManager;
use crate::toolkits::{ToolDef, Toolkit, ToolkitError};
use ownstack_engine::{
    AuditEntry, AuditLogger, PolicyDecision, PolicyEngine, Sandbox, SandboxLevel,
    ToolResult,
};

pub struct GitToolkit {
    workspace: PathBuf,
    session_id: String,
    audit_logger: AuditLogger,
    approval: Option<Arc<PolicyApprovalManager>>,
    // policy is no longer needed as a field since evaluate is static,
    // but we keep it for now if we want to add non-static state later.
    // For now, let's keep the constructor signature consistent.
    _policy: Arc<PolicyEngine>,
    sandbox: Arc<dyn Sandbox + Send + Sync>,
    provider: Arc<dyn LlmProvider + Send + Sync>,
}

impl GitToolkit {
    pub fn new(
        workspace: PathBuf,
        session_id: String,
        approval: Option<Arc<PolicyApprovalManager>>,
        policy: Arc<PolicyEngine>,
        sandbox: Arc<dyn Sandbox + Send + Sync>,
        provider: Arc<dyn LlmProvider + Send + Sync>,
    ) -> Self {
        let audit_logger = AuditLogger::new(workspace.clone());
        Self {
            workspace,
            session_id,
            audit_logger,
            approval,
            _policy: policy,
            sandbox,
            provider,
        }
    }

    fn audit(
        &self,
        command: &str,
        policy_decision: PolicyDecision,
        success: bool,
        duration_ms: u64,
    ) {
        let entry = AuditEntry {
            timestamp: String::new(),
            session_id: self.session_id.clone(),
            action: "exec".to_string(),
            command: command.to_string(),
            policy_decision,
            tool_name: "git.exec".to_string(),
            success,
            duration_ms,
            workspace: self.workspace.to_string_lossy().to_string(),
            paths_accessed: Vec::new(),
        };

        if let Err(err) = self.audit_logger.log(entry) {
            warn!("audit log failed: {}", err);
        }
    }

    async fn run_git(&self, command: &str) -> Result<ToolResult, ToolkitError> {
        let start = Instant::now();
        let full_command = format!("git {}", command);
        // PolicyEngine::evaluate is static in ownstack-engine/src/policy.rs
        let decision = PolicyEngine::evaluate(&full_command);

        match decision {
            PolicyDecision::Blocked => {
                self.audit(
                    &full_command,
                    PolicyDecision::Blocked,
                    false,
                    start.elapsed().as_millis() as u64,
                );
                Err(ToolkitError::SecurityViolation(full_command))
            }
            PolicyDecision::Ask => {
                if let Some(approval) = self.approval.as_ref() {
                    let approved = approval
                        .request(
                            full_command.clone(),
                            "Git command requires user approval".to_string(),
                            None,
                        )
                        .await;
                    if !approved {
                        self.audit(
                            &full_command,
                            PolicyDecision::Ask,
                            false,
                            start.elapsed().as_millis() as u64,
                        );
                        return Err(ToolkitError::SecurityViolation(format!(
                            "Git command denied by user: {}",
                            full_command
                        )));
                    }
                } else {
                    self.audit(
                        &full_command,
                        PolicyDecision::Ask,
                        false,
                        start.elapsed().as_millis() as u64,
                    );
                    return Err(ToolkitError::SecurityViolation(format!(
                        "Git command requires approval but UI is not connected: {}",
                        full_command
                    )));
                }

                let result = self
                    .sandbox
                    .exec(&full_command, &self.workspace, SandboxLevel::Standard)
                    .await;
                self.audit(
                    &full_command,
                    PolicyDecision::Ask,
                    result.success,
                    start.elapsed().as_millis() as u64,
                );
                Ok(result)
            }
            PolicyDecision::Auto => {
                let result = self
                    .sandbox
                    .exec(&full_command, &self.workspace, SandboxLevel::Standard)
                    .await;
                self.audit(
                    &full_command,
                    PolicyDecision::Auto,
                    result.success,
                    start.elapsed().as_millis() as u64,
                );
                Ok(result)
            }
        }
    }

    async fn status(&self) -> Result<ToolResult, ToolkitError> {
        self.run_git("status --porcelain").await
    }

    async fn diff(&self, staged: bool) -> Result<ToolResult, ToolkitError> {
        let arg = if staged { "--cached" } else { "" };
        self.run_git(&format!("diff {}", arg)).await
    }

    async fn add(&self, path: &str) -> Result<ToolResult, ToolkitError> {
        self.run_git(&format!("add {}", path)).await
    }

    async fn commit(&self, message: &str) -> Result<ToolResult, ToolkitError> {
        self.run_git(&format!("commit -m \"{}\"", message)).await
    }

    async fn push(
        &self,
        remote: &str,
        branch: &str,
    ) -> Result<ToolResult, ToolkitError> {
        self.run_git(&format!("push {} {}", remote, branch)).await
    }

    async fn pull(
        &self,
        remote: &str,
        branch: &str,
    ) -> Result<ToolResult, ToolkitError> {
        self.run_git(&format!("pull {} {}", remote, branch)).await
    }

    async fn branch_list(&self) -> Result<ToolResult, ToolkitError> {
        self.run_git("branch").await
    }

    async fn branch_create(&self, name: &str) -> Result<ToolResult, ToolkitError> {
        self.run_git(&format!("checkout -b {}", name)).await
    }

    pub async fn branch_switch(
        &self,
        name: &str,
    ) -> Result<ToolResult, ToolkitError> {
        self.run_git(&format!("checkout {}", name)).await
    }

    pub async fn suggest_commit_message(&self) -> Result<ToolResult, ToolkitError> {
        info!("Suggesting commit message (staged changes)...");
        info!("CWD: {:?}", self.workspace);

        let toplevel_res = self.run_git("rev-parse --show-toplevel").await?;
        info!("Git Toplevel: {}", toplevel_res.stdout.trim());

        let status_res = self.run_git("status --porcelain").await?;
        info!("Git Status: {}", status_res.stdout);

        let diff_res = self.diff(true).await?;
        if !diff_res.success {
            return Ok(ToolResult::failure(
                format!("Git diff failed: {}", diff_res.stderr),
                diff_res.exit_code,
            ));
        }

        info!("Diff length: {}", diff_res.stdout.len());

        if diff_res.stdout.is_empty() {
            return Ok(ToolResult::failure(
                "No changes to suggest a commit message for.".to_string(),
                None,
            ));
        }

        // Edge Case: Massive diff mitigation
        const MAX_DIFF_CHARS: usize = 50_000;
        let diff_text = if diff_res.stdout.len() > MAX_DIFF_CHARS {
            info!(
                "Truncating diff from {} to {} chars",
                diff_res.stdout.len(),
                MAX_DIFF_CHARS
            );
            format!("{}... [TRUNCATED]", &diff_res.stdout[..MAX_DIFF_CHARS])
        } else {
            diff_res.stdout.clone()
        };

        let prompt = format!(
            "Suggest a concise commit message based on these staged changes:\n\n```diff\n{}\n```",
            diff_text
        );

        let messages = vec![LlmMessage::user(prompt)];

        match self.provider.complete(messages, None, None).await {
            Ok(response) => {
                info!("LLM response received");
                if let Some(content) = response.content {
                    Ok(ToolResult::success(content.trim().to_string()))
                } else {
                    Ok(ToolResult::failure(
                        "LLM returned empty response".to_string(),
                        None,
                    ))
                }
            }
            Err(e) => {
                error!("LLM call failed: {}", e);
                Ok(ToolResult::failure(
                    format!("LLM provider error: {}", e),
                    None,
                ))
            }
        }
    }
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
                description: "Show working tree status".to_string(),
                parameters: serde_json::json!({}),
            },
            ToolDef {
                name: "git_diff".to_string(),
                description: "Show changes between commits/working tree".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "staged": { "type": "boolean" }
                    }
                }),
            },
            ToolDef {
                name: "git_add".to_string(),
                description: "Add file contents to the index".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
                    },
                    "required": ["path"]
                }),
            },
            ToolDef {
                name: "git_commit".to_string(),
                description: "Record changes to the repository".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "message": { "type": "string" }
                    },
                    "required": ["message"]
                }),
            },
            ToolDef {
                name: "git_push".to_string(),
                description: "Update remote refs along with associated objects".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "remote": { "type": "string" },
                        "branch": { "type": "string" }
                    },
                    "required": ["branch"]
                }),
            },
            ToolDef {
                name: "git_pull".to_string(),
                description: "Fetch from and integrate with another repository or a local branch".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "remote": { "type": "string" },
                        "branch": { "type": "string" }
                    },
                    "required": ["branch"]
                }),
            },
            ToolDef {
                name: "git_branch_list".to_string(),
                description: "List branches".to_string(),
                parameters: serde_json::json!({}),
            },
            ToolDef {
                name: "git_branch_create".to_string(),
                description: "Create a new branch".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" }
                    },
                    "required": ["name"]
                }),
            },
            ToolDef {
                name: "git_suggest_commit".to_string(),
                description: "AI-assisted commit message suggestion (staged changes only)".to_string(),
                parameters: serde_json::json!({}),
            },
        ]
    }

    async fn execute(
        &self,
        name: &str,
        args: serde_json::Value,
    ) -> Result<ToolResult, ToolkitError> {
        info!("GitToolkit::execute: {}", name);
        match name {
            "git_status" => self.status().await,
            "git_diff" => {
                let staged = args["staged"].as_bool().unwrap_or(false);
                self.diff(staged).await
            }
            "git_add" => {
                let path =
                    args["path"].as_str().ok_or(ToolkitError::InvalidArguments(
                        "Missing 'path' argument".to_string(),
                    ))?;
                self.add(path).await
            }
            "git_commit" => {
                let message = args["message"].as_str().ok_or(
                    ToolkitError::InvalidArguments(
                        "Missing 'message' argument".to_string(),
                    ),
                )?;
                self.commit(message).await
            }
            "git_push" => {
                let remote = args["remote"].as_str().unwrap_or("origin");
                let branch = args["branch"].as_str().ok_or(
                    ToolkitError::InvalidArguments(
                        "Missing 'branch' argument".to_string(),
                    ),
                )?;
                self.push(remote, branch).await
            }
            "git_pull" => {
                let remote = args["remote"].as_str().unwrap_or("origin");
                let branch = args["branch"].as_str().ok_or(
                    ToolkitError::InvalidArguments(
                        "Missing 'branch' argument".to_string(),
                    ),
                )?;
                self.pull(remote, branch).await
            }
            "git_branch_list" => self.branch_list().await,
            "git_branch_create" => {
                let name =
                    args["name"].as_str().ok_or(ToolkitError::InvalidArguments(
                        "Missing 'name' argument".to_string(),
                    ))?;
                self.branch_create(name).await
            }
            "git_suggest_commit" => self.suggest_commit_message().await,
            _ => Err(ToolkitError::ToolNotFound(name.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{
        FinishReason, LlmMessage, LlmProvider, LlmResponse, ProviderError,
    };
    use std::fs;
    use std::process::Command;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    struct MockProvider {
        last_message: Arc<Mutex<String>>,
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn complete(
            &self,
            m: Vec<LlmMessage>,
            _t: Option<Vec<crate::provider::ToolDefinition>>,
            _model_override: Option<String>,
        ) -> Result<LlmResponse, ProviderError> {
            if let Some(msg) = m.first() {
                let mut last = self.last_message.lock().unwrap();
                *last = msg.content.get_text();
            }
            Ok(LlmResponse {
                content: Some("suggested commit".to_string()),
                tool_calls: vec![],
                finish_reason: FinishReason::Stop,
                usage: None,
            })
        }

        fn name(&self) -> &str {
            "mock-provider"
        }
    }

    fn init_git_repo(path: &std::path::Path) {
        let _ = Command::new("git").arg("init").current_dir(path).status();

        // Configure user for commit
        let _ = Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(path)
            .status();

        let _ = Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(path)
            .status();
    }

    #[tokio::test]
    async fn test_git_status() {
        let dir = tempdir().unwrap();
        let path = dir.path();
        init_git_repo(path);

        fs::write(path.join("test.txt"), "hello").unwrap();

        let policy = Arc::new(PolicyEngine);
        let sandbox = Arc::new(ownstack_engine::ProcessSandbox);
        let provider = Arc::new(MockProvider {
            last_message: Arc::new(Mutex::new(String::new())),
        });
        let git = GitToolkit::new(
            path.to_path_buf(),
            "test".to_string(),
            None,
            policy,
            sandbox,
            provider,
        );

        let result = git.status().await.unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("?? test.txt"));
    }

    #[tokio::test]
    async fn test_git_suggest_commit() {
        let dir = tempdir().unwrap();
        let path = dir.path();
        init_git_repo(path);

        fs::write(path.join("test.txt"), "hello").unwrap();

        let _ = Command::new("git")
            .args(["add", "test.txt"])
            .current_dir(path)
            .status();

        let policy = Arc::new(PolicyEngine);
        let sandbox = Arc::new(ownstack_engine::ProcessSandbox);
        let provider = Arc::new(MockProvider {
            last_message: Arc::new(Mutex::new(String::new())),
        });
        let git = GitToolkit::new(
            path.to_path_buf(),
            "test".to_string(),
            None,
            policy,
            sandbox,
            provider,
        );

        let result = git.suggest_commit_message().await.unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("suggested commit"));
    }

    #[tokio::test]
    async fn test_massive_diff_truncation() {
        let dir = tempdir().unwrap();
        let path = dir.path();
        init_git_repo(path);

        // Create a massive file (200k chars)
        let content = "a".repeat(200_000);
        fs::write(path.join("massive.txt"), &content).unwrap();

        let _ = Command::new("git")
            .args(["add", "massive.txt"])
            .current_dir(path)
            .status()
            .expect("Failed to execute git add");

        let policy = Arc::new(PolicyEngine);
        let sandbox = Arc::new(ownstack_engine::ProcessSandbox);
        let last_message = Arc::new(Mutex::new(String::new()));
        let provider = Arc::new(MockProvider {
            last_message: last_message.clone(),
        });
        let git = GitToolkit::new(
            path.to_path_buf(),
            "test".to_string(),
            None,
            policy,
            sandbox.clone(),
            provider,
        );

        let result = git.suggest_commit_message().await.unwrap();
        assert!(result.success);

        let message = last_message.lock().unwrap();
        assert!(message.contains("[TRUNCATED]"));
        // formatting adds some overhead, so length should be around 50k + prompt overhead
        // But definitely less than 200k
        assert!(message.len() < 100_000);
        assert!(message.len() > 40_000); // Should be at least 50k chars of diff
    }
}
