//! Mission Compiler — Transforms natural language into a MissionSpec.
//!
//! Uses an LLM to compile a user prompt into a formal technical contract
//! with security modes, execution strategy, oracles, and budget.

use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::provider::{
    LlmMessage, LlmProvider, MessageContent, ProviderOptions, Role,
};

use super::models::{ExecutionStrategy, MissionMode, MissionSpec, Permission};

// ─── Compiler Prompt ─────────────────────────────────────────────

const COMPILER_SYSTEM_PROMPT: &str = r#"You are the OwnStack Mission Compiler.
Your goal is to transform a natural language user request into a rigorous technical contract (MissionSpec).

STRICT COMPILATION RULES:
1. MODE SELECTION:
   - 'static_read': Pure file reading, grep, and parsing. No tool execution. Use for initial audits.
   - 'safe_tooling': Allows non-destructive tools like LSP or linters.
   - 'dynamic_exec': Execution of tests or code in an ephemeral sandbox. Standard for most missions.
   - 'hypothetical': Planning only, no execution.

2. STRATEGY: For 'dynamic_exec', always specify 'ephemeral_branch' or 'patch_log' to ensure safety.

3. ORACLES: Define technical truth sources (e.g., "cargo test", "npm run build").

4. STOP CONDITIONS: Be explicit (e.g., "failed_tests", "unresolved_imports").

RESPONSE FORMAT:
You MUST return a valid JSON object matching this schema:
{
  "mode": "static_read" | "safe_tooling" | "dynamic_exec" | "hypothetical",
  "strategy": "ephemeral_branch" | "patch_log" | "dry_run",
  "objectives": ["string"],
  "scope": ["string (file/dir paths)"],
  "permissions": ["fs_read", "fs_write", "exec", "network", "git", "lsp"],
  "oracles": ["string (verification commands)"],
  "stop_conditions": ["string"],
  "output_format": ["markdown_matrix", "diff_patch", "json_report"],
  "budget_tokens": <number or null>
}

Return ONLY the JSON, no markdown, no extra text."#;

// ─── Preflight Checks ───────────────────────────────────────────

/// Detect available tools in the workspace.
fn run_preflight(workspace: &PathBuf) -> std::collections::HashMap<String, bool> {
    let mut checks = std::collections::HashMap::new();

    checks.insert("cargo".to_string(), workspace.join("Cargo.toml").exists());
    checks.insert("npm".to_string(), workspace.join("package.json").exists());
    checks.insert(
        "pytest".to_string(),
        workspace.join("pytest.ini").exists()
            || workspace.join("pyproject.toml").exists()
            || workspace.join("setup.py").exists(),
    );
    checks.insert("git".to_string(), workspace.join(".git").exists());

    checks
}

// ─── Mission Compiler ───────────────────────────────────────────

/// Compiles natural language prompts into typed MissionSpec contracts.
pub struct MissionCompiler {
    provider: Arc<dyn LlmProvider>,
    workspace: PathBuf,
}

impl MissionCompiler {
    pub fn new(provider: Arc<dyn LlmProvider>, workspace: PathBuf) -> Self {
        Self {
            provider,
            workspace,
        }
    }

    /// Compile a user prompt into a MissionSpec.
    pub async fn compile(&self, user_prompt: &str) -> Result<MissionSpec, String> {
        let preflight = run_preflight(&self.workspace);

        let input = format!(
            "USER REQUEST: {user_prompt}\n\n\
             PREFLIGHT DATA (Available tools): {}\n\n\
             WORKSPACE: {}",
            serde_json::to_string(&preflight).unwrap_or_default(),
            self.workspace.display()
        );

        let messages = vec![
            LlmMessage {
                role: Role::System,
                content: MessageContent::Text(COMPILER_SYSTEM_PROMPT.to_string()),
                tool_calls: None,
                tool_call_id: None,
            },
            LlmMessage {
                role: Role::User,
                content: MessageContent::Text(input),
                tool_calls: None,
                tool_call_id: None,
            },
        ];

        let response = self
            .provider
            .complete(messages, None, ProviderOptions::default())
            .await
            .map_err(|e| format!("LLM call failed: {e}"))?;

        let content = response.content.unwrap_or_default();
        info!("MissionCompiler: received {} bytes from LLM", content.len());

        let json_str = extract_json(&content);

        match serde_json::from_str::<MissionSpec>(&json_str) {
            Ok(mut spec) => {
                spec.preflight_checks = preflight;
                debug!("MissionCompiler: compiled spec with mode {:?}", spec.mode);
                Ok(spec)
            }
            Err(e) => {
                warn!(
                    "MissionCompiler: JSON parse failed: {e}, falling back to safe defaults"
                );
                Ok(fallback_spec(user_prompt, preflight))
            }
        }
    }
}

/// Extract JSON from potentially markdown-wrapped responses.
pub fn extract_json(content: &str) -> String {
    let trimmed = content.trim();

    // Try ```json ... ```
    if let Some(start) = trimmed.find("```json") {
        if let Some(end) = trimmed[start + 7..].find("```") {
            return trimmed[start + 7..start + 7 + end].trim().to_string();
        }
    }

    // Try ``` ... ```
    if let Some(start) = trimmed.find("```") {
        if let Some(end) = trimmed[start + 3..].find("```") {
            return trimmed[start + 3..start + 3 + end].trim().to_string();
        }
    }

    // Assume raw JSON
    trimmed.to_string()
}

/// Safe fallback when compilation fails.
fn fallback_spec(
    prompt: &str,
    preflight: std::collections::HashMap<String, bool>,
) -> MissionSpec {
    MissionSpec {
        mode: MissionMode::StaticRead,
        strategy: ExecutionStrategy::DryRun,
        objectives: vec![format!("Fallback: {}", &prompt[..prompt.len().min(100)])],
        scope: vec![".".to_string()],
        permissions: vec![Permission::FsRead],
        oracles: Vec::new(),
        stop_conditions: vec!["manual_review".to_string()],
        output_format: vec!["error_log".to_string()],
        budget_tokens: Some(10_000),
        preflight_checks: preflight,
    }
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_plain() {
        let json = extract_json(r#"{"mode": "static_read"}"#);
        assert!(json.contains("static_read"));
    }

    #[test]
    fn test_extract_json_markdown_wrapped() {
        let input =
            "Here's the spec:\n```json\n{\"mode\": \"dynamic_exec\"}\n```\nDone.";
        let json = extract_json(input);
        assert!(json.contains("dynamic_exec"));
    }

    #[test]
    fn test_preflight_detects_cargo() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();

        let checks = run_preflight(&dir.path().to_path_buf());
        assert_eq!(checks.get("cargo"), Some(&true));
        assert_eq!(checks.get("npm"), Some(&false));
    }

    #[test]
    fn test_fallback_spec_is_safe() {
        let spec = fallback_spec(
            "do something dangerous",
            std::collections::HashMap::new(),
        );
        assert_eq!(spec.mode, MissionMode::StaticRead);
        assert_eq!(spec.strategy, ExecutionStrategy::DryRun);
        assert!(spec.permissions.contains(&Permission::FsRead));
        assert!(!spec.permissions.contains(&Permission::Exec));
    }
}
