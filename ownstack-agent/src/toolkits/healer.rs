//! Self-Healing Agent — Ported from Python healer.py
//!
//! Detects failures, generates fixes, applies them in sandbox,
//! and validates before suggesting. All operations go through
//! the security pipeline (PolicyEngine + Sandbox).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info, warn};

use ownstack_engine::{
    PolicyDecision, PolicyEngine, ProcessSandbox, Sandbox, SandboxLevel,
};

use crate::provider::{LlmMessage, LlmProvider};

use super::{ToolDef, ToolResult, Toolkit, ToolkitError};

// ─── Failure Types ─────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FailureType {
    TestFailure,
    ImportError,
    SyntaxError,
    TypeError,
    DependencyMissing,
    ConfigError,
    RuntimeError,
    Unknown,
}

impl std::fmt::Display for FailureType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TestFailure => write!(f, "test_failure"),
            Self::ImportError => write!(f, "import_error"),
            Self::SyntaxError => write!(f, "syntax_error"),
            Self::TypeError => write!(f, "type_error"),
            Self::DependencyMissing => write!(f, "dependency_missing"),
            Self::ConfigError => write!(f, "config_error"),
            Self::RuntimeError => write!(f, "runtime_error"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Failure {
    pub failure_type: FailureType,
    pub file_path: Option<String>,
    pub line_number: Option<u32>,
    pub error_message: String,
    pub suggested_fixes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealingAttempt {
    pub failure: Failure,
    pub fix_applied: String,
    pub success: bool,
    pub verification_output: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealingSession {
    pub session_id: String,
    pub original_command: String,
    pub original_output: String,
    pub failures_detected: Vec<Failure>,
    pub attempts: Vec<HealingAttempt>,
    pub healed: bool,
    pub total_attempts: u32,
    pub max_attempts: u32,
}

// ─── Failure Analyzer ──────────────────────────────────────────────

pub struct FailureAnalyzer;

impl FailureAnalyzer {
    pub fn analyze(output: &str, exit_code: i32) -> Vec<Failure> {
        if exit_code == 0 {
            return Vec::new();
        }

        let mut failures = Vec::new();

        // ─── Python Import Errors ────────────────────────────────
        for pattern in &[
            "ModuleNotFoundError: No module named",
            "ImportError: cannot import name",
            "Error: Cannot find module",
        ] {
            if output.contains(pattern) {
                let module = Self::extract_quoted(output, pattern);
                failures.push(Failure {
                    failure_type: FailureType::ImportError,
                    file_path: Self::extract_file_path(output),
                    line_number: Self::extract_line_number(output),
                    error_message: format!("{} '{}'", pattern, module),
                    suggested_fixes: vec![format!("pip install {}", module)],
                });
            }
        }

        // ─── Python Syntax Errors ────────────────────────────────
        if output.contains("SyntaxError:") || output.contains("IndentationError:") {
            failures.push(Failure {
                failure_type: FailureType::SyntaxError,
                file_path: Self::extract_file_path(output),
                line_number: Self::extract_line_number(output),
                error_message: Self::extract_after(output, "SyntaxError:")
                    .unwrap_or_else(|| {
                        Self::extract_after(output, "IndentationError:")
                            .unwrap_or_default()
                    }),
                suggested_fixes: vec![
                    "# Syntax error requires code review".to_string()
                ],
            });
        }

        // ─── Python Type Errors ──────────────────────────────────
        if output.contains("TypeError:") || output.contains("AttributeError:") {
            failures.push(Failure {
                failure_type: FailureType::TypeError,
                file_path: Self::extract_file_path(output),
                line_number: Self::extract_line_number(output),
                error_message: Self::extract_after(output, "TypeError:")
                    .unwrap_or_else(|| {
                        Self::extract_after(output, "AttributeError:")
                            .unwrap_or_default()
                    }),
                suggested_fixes: Vec::new(),
            });
        }

        // ─── Rust Compiler Errors ────────────────────────────────
        if output.contains("error[E") {
            let rust_file = Self::extract_rust_file_path(output);
            let rust_line = Self::extract_rust_line_number(output);
            let error_code = Self::extract_rust_error_code(output);
            let error_msg = Self::extract_after(output, "error[E")
                .map(|s| format!("error[E{}", s))
                .unwrap_or_else(|| "Rust compilation error".to_string());

            let mut suggested = Vec::new();
            // Missing crate → suggest adding to Cargo.toml
            if output.contains("unresolved import")
                || output.contains("can't find crate")
            {
                if let Some(crate_name) = Self::extract_rust_crate_name(output) {
                    suggested.push(format!("cargo add {}", crate_name));
                }
            }
            // Missing derive or trait
            if output.contains("doesn't implement")
                || output.contains("not implemented")
            {
                suggested.push(
                    "# Implement the required trait or add a derive macro"
                        .to_string(),
                );
            }

            failures.push(Failure {
                failure_type: if error_code.as_deref() == Some("E0433")
                    || error_code.as_deref() == Some("E0432")
                {
                    FailureType::ImportError
                } else if error_code.as_deref() == Some("E0308")
                    || error_code.as_deref() == Some("E0277")
                {
                    FailureType::TypeError
                } else {
                    FailureType::SyntaxError
                },
                file_path: rust_file,
                line_number: rust_line,
                error_message: error_msg,
                suggested_fixes: suggested,
            });
        }

        // Rust: missing dependency in Cargo.toml
        if output.contains("no matching package named") {
            let pkg = Self::extract_after(output, "no matching package named `")
                .map(|s| s.trim_end_matches('`').to_string())
                .unwrap_or_default();
            failures.push(Failure {
                failure_type: FailureType::DependencyMissing,
                file_path: None,
                line_number: None,
                error_message: format!("Missing Rust crate: {}", pkg),
                suggested_fixes: vec![format!("cargo add {}", pkg)],
            });
        }

        // ─── JavaScript / TypeScript Errors ──────────────────────
        if output.contains("ReferenceError:") || output.contains("is not defined") {
            failures.push(Failure {
                failure_type: FailureType::RuntimeError,
                file_path: Self::extract_js_file_path(output),
                line_number: Self::extract_js_line_number(output),
                error_message: Self::extract_after(output, "ReferenceError:")
                    .unwrap_or_else(|| "ReferenceError".to_string()),
                suggested_fixes: Vec::new(),
            });
        }

        // TypeScript compilation errors (TSxxxx)
        if output.contains("error TS") {
            failures.push(Failure {
                failure_type: FailureType::SyntaxError,
                file_path: Self::extract_ts_file_path(output),
                line_number: Self::extract_ts_line_number(output),
                error_message: Self::extract_after(output, "error TS")
                    .map(|s| format!("TS{}", s))
                    .unwrap_or_else(|| "TypeScript compilation error".to_string()),
                suggested_fixes: Vec::new(),
            });
        }

        // Node.js MODULE_NOT_FOUND
        if output.contains("MODULE_NOT_FOUND")
            || output.contains("Cannot find module")
        {
            let module = Self::extract_quoted(output, "Cannot find module");
            failures.push(Failure {
                failure_type: FailureType::DependencyMissing,
                file_path: None,
                line_number: None,
                error_message: format!("Missing Node module: '{}'", module),
                suggested_fixes: vec![format!("npm install {}", module)],
            });
        }

        // ─── Go Errors ──────────────────────────────────────────
        if output.contains("undefined:") && output.contains(".go:") {
            failures.push(Failure {
                failure_type: FailureType::SyntaxError,
                file_path: Self::extract_go_file_path(output),
                line_number: Self::extract_go_line_number(output),
                error_message: Self::extract_after(output, "undefined:")
                    .map(|s| format!("undefined: {}", s))
                    .unwrap_or_else(|| "Go compilation error".to_string()),
                suggested_fixes: Vec::new(),
            });
        }

        if output.contains("cannot find package") {
            let pkg = Self::extract_quoted(output, "cannot find package");
            failures.push(Failure {
                failure_type: FailureType::DependencyMissing,
                file_path: None,
                line_number: None,
                error_message: format!("Missing Go package: '{}'", pkg),
                suggested_fixes: vec![format!("go get {}", pkg)],
            });
        }

        // ─── Test Failures (multi-language) ──────────────────────
        let has_test_failure = output.contains("FAILED")
            || output.contains("test result: FAILED")
            || output.contains("failures:")
            || (output.contains("FAIL") && output.contains(".go:"));
        if has_test_failure {
            failures.push(Failure {
                failure_type: FailureType::TestFailure,
                file_path: Self::extract_file_path(output),
                line_number: Self::extract_line_number(output),
                error_message: "Test failure detected".to_string(),
                suggested_fixes: vec![
                    "# Test failure requires LLM analysis".to_string()
                ],
            });
        }

        // ─── Dependency Missing (multi-language) ─────────────────
        if output.contains("pip install")
            || output.contains("npm install")
            || output.contains("cargo add")
            || output.contains("go get")
            || output.contains("Could not find a version")
        {
            failures.push(Failure {
                failure_type: FailureType::DependencyMissing,
                file_path: None,
                line_number: None,
                error_message: "Missing dependency".to_string(),
                suggested_fixes: Self::extract_install_commands(output),
            });
        }

        // Generic fallback
        if failures.is_empty() {
            failures.push(Failure {
                failure_type: FailureType::Unknown,
                file_path: Self::extract_file_path(output),
                line_number: Self::extract_line_number(output),
                error_message: output.chars().take(500).collect(),
                suggested_fixes: Vec::new(),
            });
        }

        failures
    }

    fn extract_quoted(output: &str, after: &str) -> String {
        if let Some(pos) = output.find(after) {
            let rest = &output[pos + after.len()..];
            if let Some(start) = rest.find('\'') {
                let inner = &rest[start + 1..];
                if let Some(end) = inner.find('\'') {
                    return inner[..end].to_string();
                }
            }
        }
        String::new()
    }

    fn extract_after(output: &str, marker: &str) -> Option<String> {
        output.find(marker).map(|pos| {
            let rest = &output[pos + marker.len()..];
            let end = rest.find('\n').unwrap_or(rest.len());
            rest[..end].trim().to_string()
        })
    }

    fn extract_file_path(output: &str) -> Option<String> {
        // Match Python: File "path", line N
        if let Some(pos) = output.find("File \"") {
            let rest = &output[pos + 6..];
            if let Some(end) = rest.find('"') {
                return Some(rest[..end].to_string());
            }
        }
        None
    }

    fn extract_line_number(output: &str) -> Option<u32> {
        if let Some(pos) = output.find(", line ") {
            let rest = &output[pos + 7..];
            let num: String =
                rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            return num.parse().ok();
        }
        None
    }

    fn extract_install_commands(output: &str) -> Vec<String> {
        let mut cmds = Vec::new();
        for line in output.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("pip install")
                || trimmed.starts_with("npm install")
                || trimmed.starts_with("cargo add")
                || trimmed.starts_with("go get")
            {
                cmds.push(trimmed.to_string());
            }
        }
        cmds
    }

    // ─── Rust-specific extractors ────────────────────────────────

    fn extract_rust_file_path(output: &str) -> Option<String> {
        // Rust errors: --> src/main.rs:15:5
        for line in output.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("-->") {
                let rest = trimmed.trim_start_matches("-->").trim();
                if let Some(colon_pos) = rest.find(':') {
                    return Some(rest[..colon_pos].to_string());
                }
            }
        }
        None
    }

    fn extract_rust_line_number(output: &str) -> Option<u32> {
        for line in output.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("-->") {
                let rest = trimmed.trim_start_matches("-->").trim();
                let parts: Vec<&str> = rest.split(':').collect();
                if parts.len() >= 2 {
                    return parts[1].parse().ok();
                }
            }
        }
        None
    }

    fn extract_rust_error_code(output: &str) -> Option<String> {
        // error[E0308]: mismatched types
        if let Some(start) = output.find("error[E") {
            let rest = &output[start + 6..]; // skip "error["
            if let Some(end) = rest.find(']') {
                return Some(rest[..end].to_string());
            }
        }
        None
    }

    fn extract_rust_crate_name(output: &str) -> Option<String> {
        // "unresolved import `foo`" or "can't find crate for `foo`"
        for pattern in &["unresolved import `", "can't find crate for `"] {
            if let Some(pos) = output.find(pattern) {
                let rest = &output[pos + pattern.len()..];
                if let Some(end) = rest.find('`') {
                    let name = &rest[..end];
                    // Take the top-level crate name (before ::)
                    let crate_name = name.split("::").next().unwrap_or(name);
                    return Some(crate_name.to_string());
                }
            }
        }
        None
    }

    // ─── JavaScript / TypeScript extractors ──────────────────────

    fn extract_js_file_path(output: &str) -> Option<String> {
        // Node.js: at Object.<anonymous> (/path/to/file.js:10:5)
        // or: /path/to/file.js:10
        for line in output.lines() {
            let trimmed = line.trim();
            if let Some(paren_start) = trimmed.find('(') {
                let rest = &trimmed[paren_start + 1..];
                if let Some(colon) = rest.find(':') {
                    let path = &rest[..colon];
                    if path.ends_with(".js")
                        || path.ends_with(".ts")
                        || path.ends_with(".mjs")
                    {
                        return Some(path.to_string());
                    }
                }
            }
        }
        None
    }

    fn extract_js_line_number(output: &str) -> Option<u32> {
        for line in output.lines() {
            let trimmed = line.trim();
            if let Some(paren_start) = trimmed.find('(') {
                let rest = &trimmed[paren_start + 1..];
                let parts: Vec<&str> = rest.split(':').collect();
                if parts.len() >= 2 {
                    return parts[1].parse().ok();
                }
            }
        }
        None
    }

    fn extract_ts_file_path(output: &str) -> Option<String> {
        // TypeScript: src/app.ts(15,3): error TS2304
        for line in output.lines() {
            let trimmed = line.trim();
            if trimmed.contains("error TS") {
                if let Some(paren) = trimmed.find('(') {
                    return Some(trimmed[..paren].to_string());
                }
            }
        }
        None
    }

    fn extract_ts_line_number(output: &str) -> Option<u32> {
        for line in output.lines() {
            let trimmed = line.trim();
            if trimmed.contains("error TS") {
                if let Some(paren) = trimmed.find('(') {
                    let rest = &trimmed[paren + 1..];
                    let num: String =
                        rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                    return num.parse().ok();
                }
            }
        }
        None
    }

    // ─── Go extractors ──────────────────────────────────────────

    fn extract_go_file_path(output: &str) -> Option<String> {
        // Go: ./main.go:15:5: undefined: foo
        for line in output.lines() {
            let trimmed = line.trim();
            if trimmed.contains(".go:") {
                if let Some(colon) = trimmed.find(".go:") {
                    return Some(trimmed[..colon + 3].to_string());
                }
            }
        }
        None
    }

    fn extract_go_line_number(output: &str) -> Option<u32> {
        for line in output.lines() {
            let trimmed = line.trim();
            if let Some(go_ext) = trimmed.find(".go:") {
                let rest = &trimmed[go_ext + 4..]; // skip ".go:"
                let num: String =
                    rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                return num.parse().ok();
            }
        }
        None
    }
}

// ─── Self-Healing Engine ───────────────────────────────────────────

pub struct HealerToolkit {
    workspace: PathBuf,
    provider: Option<Arc<dyn LlmProvider + Send + Sync>>,
}

impl HealerToolkit {
    pub fn new(
        workspace: PathBuf,
        provider: Option<Arc<dyn LlmProvider + Send + Sync>>,
    ) -> Self {
        Self {
            workspace,
            provider,
        }
    }

    async fn llm_suggest_fixes(
        &self,
        command: &str,
        failure: &Failure,
        output: &str,
    ) -> Vec<String> {
        let Some(provider) = self.provider.as_ref() else {
            return Vec::new();
        };

        let prompt = format!(
            "You are a remediation assistant. Suggest safe shell commands to fix a failing development command.\n\
Return ONLY JSON: {{\"suggested_fixes\": [\"cmd1\", \"cmd2\"]}}.\n\
Do not include prose.\n\
The commands must stay in the current workspace and avoid privileged/system operations.\n\n\
Original command:\n{}\n\n\
Failure type:\n{}\n\n\
Failure message:\n{}\n\n\
Output snippet:\n{}",
            command,
            failure.failure_type,
            failure.error_message,
            output.chars().take(4000).collect::<String>()
        );

        let messages = vec![
            LlmMessage::system(
                "Return strict JSON with key 'suggested_fixes' only.",
            ),
            LlmMessage::user(prompt),
        ];

        let response = match provider.complete(messages, None, None).await {
            Ok(resp) => resp,
            Err(err) => {
                warn!("Healer LLM fallback failed: {}", err);
                return Vec::new();
            }
        };

        let content = response.content.unwrap_or_default();
        let parsed = serde_json::from_str::<LlmFixResponse>(&content);
        match parsed {
            Ok(payload) => payload
                .suggested_fixes
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .take(8)
                .collect(),
            Err(err) => {
                warn!("Healer LLM fallback returned invalid JSON: {}", err);
                Vec::new()
            }
        }
    }

    /// Run the self-healing loop for a failing command
    pub async fn heal(&self, command: &str, max_attempts: u32) -> HealingSession {
        let session_id = format!(
            "heal-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );

        let mut session = HealingSession {
            session_id,
            original_command: command.to_string(),
            original_output: String::new(),
            failures_detected: Vec::new(),
            attempts: Vec::new(),
            healed: false,
            total_attempts: 0,
            max_attempts,
        };

        // Initial run
        let sandbox = ProcessSandbox;
        let initial = sandbox
            .exec(command, &self.workspace, SandboxLevel::Standard)
            .await;
        session.original_output = format!("{}\n{}", initial.stdout, initial.stderr);

        if initial.success {
            session.healed = true;
            return session;
        }

        // Analyze failures
        session.failures_detected =
            FailureAnalyzer::analyze(&session.original_output, 1);

        info!(
            "Healer: {} failures detected for: {}",
            session.failures_detected.len(),
            command
        );

        // Healing loop
        let mut applied_fixes: HashSet<String> = HashSet::new();

        for _ in 0..max_attempts {
            session.total_attempts += 1;

            if session.failures_detected.is_empty() {
                break;
            }

            let failure = session.failures_detected[0].clone();
            let mut candidate_fixes = failure.suggested_fixes.clone();
            if candidate_fixes.is_empty()
                || matches!(
                    failure.failure_type,
                    FailureType::Unknown | FailureType::TestFailure
                )
            {
                let llm_fixes = self
                    .llm_suggest_fixes(command, &failure, &session.original_output)
                    .await;
                candidate_fixes.extend(llm_fixes);
            }

            let fix = candidate_fixes
                .iter()
                .find(|f| !applied_fixes.contains(*f))
                .cloned();

            let fix = match fix {
                Some(f) => f,
                None => {
                    info!("Healer: no new fixes available");
                    break;
                }
            };

            // Check policy for the fix command.
            let decision = PolicyEngine::evaluate(&fix);
            match decision {
                PolicyDecision::Blocked => {
                    warn!("Healer: fix blocked by policy: {}", fix);
                    break;
                }
                PolicyDecision::Ask => {
                    warn!(
                        "Healer: fix requires approval and will not be auto-executed: {}",
                        fix
                    );
                    break;
                }
                PolicyDecision::Auto => {}
            }

            applied_fixes.insert(fix.clone());
            let start = std::time::Instant::now();

            // Apply fix
            let fix_result = sandbox
                .exec(&fix, &self.workspace, SandboxLevel::Standard)
                .await;
            debug!(
                "Healer: applied fix '{}' → success={}",
                fix, fix_result.success
            );

            // Re-run original command
            let verify = sandbox
                .exec(command, &self.workspace, SandboxLevel::Standard)
                .await;
            let verify_output = format!("{}\n{}", verify.stdout, verify.stderr);

            let attempt = HealingAttempt {
                failure: failure.clone(),
                fix_applied: fix.clone(),
                success: verify.success,
                verification_output: verify_output.clone(),
                duration_ms: start.elapsed().as_millis() as u64,
            };
            session.attempts.push(attempt);

            if verify.success {
                session.healed = true;
                info!("Healer: FIXED with '{}'", fix);
                break;
            }

            // Re-analyze
            session.failures_detected = FailureAnalyzer::analyze(&verify_output, 1);
        }

        session
    }
}

#[derive(Deserialize)]
struct HealArgs {
    command: String,
    #[serde(default = "default_max_attempts")]
    max_attempts: u32,
}

#[derive(Deserialize)]
struct LlmFixResponse {
    #[serde(default)]
    suggested_fixes: Vec<String>,
}

fn default_max_attempts() -> u32 {
    5
}

#[async_trait]
impl Toolkit for HealerToolkit {
    fn name(&self) -> &str {
        "healer"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![ToolDef {
            name: "heal".to_string(),
            description: "Run a command and automatically detect + fix failures"
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Command to heal (e.g. 'pytest tests/')"
                    },
                    "max_attempts": {
                        "type": "integer",
                        "description": "Max healing attempts (default: 5)"
                    }
                },
                "required": ["command"]
            }),
        }]
    }

    async fn execute(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<ToolResult, ToolkitError> {
        match tool_name {
            "heal" => {
                let parsed: HealArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                let session = self.heal(&parsed.command, parsed.max_attempts).await;
                let summary = serde_json::to_string_pretty(&session)
                    .unwrap_or_else(|_| format!("healed: {}", session.healed));
                Ok(ToolResult::success(summary))
            }
            _ => Err(ToolkitError::ToolNotFound(tool_name.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{
        FinishReason, LlmProvider, LlmResponse, ProviderError, ToolDefinition,
    };
    use async_trait::async_trait;
    use std::path::PathBuf;
    use std::sync::Arc;

    // ─── Python errors (existing behavior) ──────────────────────
    #[test]
    fn test_python_import_error() {
        let output = "ModuleNotFoundError: No module named 'flask'";
        let failures = FailureAnalyzer::analyze(output, 1);
        assert!(!failures.is_empty());
        assert_eq!(failures[0].failure_type, FailureType::ImportError);
        assert!(failures[0].suggested_fixes[0].contains("pip install"));
    }

    #[test]
    fn test_python_syntax_error() {
        let output = "  File \"app.py\", line 10\n    def broken(\nSyntaxError: unexpected EOF while parsing";
        let failures = FailureAnalyzer::analyze(output, 1);
        assert!(failures
            .iter()
            .any(|f| f.failure_type == FailureType::SyntaxError));
    }

    #[test]
    fn test_python_type_error() {
        let output = "TypeError: unsupported operand type(s) for +: 'int' and 'str'";
        let failures = FailureAnalyzer::analyze(output, 1);
        assert!(failures
            .iter()
            .any(|f| f.failure_type == FailureType::TypeError));
    }

    // ─── Rust errors ────────────────────────────────────────────
    #[test]
    fn test_rust_compilation_error() {
        let output = r#"error[E0308]: mismatched types
 --> src/main.rs:15:5
  |
15 |     foo(x)
  |     ^^^^^^ expected `u32`, found `&str`"#;
        let failures = FailureAnalyzer::analyze(output, 1);
        assert!(failures
            .iter()
            .any(|f| f.failure_type == FailureType::TypeError));
        assert_eq!(failures[0].file_path.as_deref(), Some("src/main.rs"));
        assert_eq!(failures[0].line_number, Some(15));
    }

    #[test]
    fn test_rust_unresolved_import() {
        let output = r#"error[E0432]: unresolved import `tokio`
 --> src/main.rs:1:5
  |
1 | use tokio::runtime;
  |     ^^^^^ could not find `tokio` in the crate root"#;
        let failures = FailureAnalyzer::analyze(output, 1);
        assert!(failures
            .iter()
            .any(|f| f.failure_type == FailureType::ImportError));
        assert!(failures[0]
            .suggested_fixes
            .iter()
            .any(|f| f.contains("cargo add")));
    }

    #[test]
    fn test_rust_missing_crate() {
        let output = "error: no matching package named `serde_yaml` found";
        let failures = FailureAnalyzer::analyze(output, 1);
        assert!(failures
            .iter()
            .any(|f| f.failure_type == FailureType::DependencyMissing));
    }

    #[test]
    fn test_rust_error_code_extraction() {
        assert_eq!(
            FailureAnalyzer::extract_rust_error_code(
                "error[E0308]: mismatched types"
            ),
            Some("E0308".to_string())
        );
        assert_eq!(
            FailureAnalyzer::extract_rust_error_code("no error code here"),
            None
        );
    }

    #[test]
    fn test_rust_file_path_extraction() {
        let output = " --> src/lib.rs:42:10\n  |";
        assert_eq!(
            FailureAnalyzer::extract_rust_file_path(output),
            Some("src/lib.rs".to_string())
        );
    }

    // ─── JavaScript / TypeScript errors ─────────────────────────
    #[test]
    fn test_js_reference_error() {
        let output = "ReferenceError: foo is not defined\n    at Object.<anonymous> (/app/index.js:5:1)";
        let failures = FailureAnalyzer::analyze(output, 1);
        assert!(failures
            .iter()
            .any(|f| f.failure_type == FailureType::RuntimeError));
    }

    #[test]
    fn test_ts_compilation_error() {
        let output = "src/app.ts(15,3): error TS2304: Cannot find name 'foo'.";
        let failures = FailureAnalyzer::analyze(output, 1);
        assert!(failures
            .iter()
            .any(|f| f.failure_type == FailureType::SyntaxError));
        assert_eq!(failures[0].file_path.as_deref(), Some("src/app.ts"));
        assert_eq!(failures[0].line_number, Some(15));
    }

    #[test]
    fn test_node_module_not_found() {
        let output = "Error: Cannot find module 'express'\nRequire stack:\n- /app/index.js\ncode: 'MODULE_NOT_FOUND'";
        let failures = FailureAnalyzer::analyze(output, 1);
        assert!(failures
            .iter()
            .any(|f| f.failure_type == FailureType::DependencyMissing));
        assert!(failures
            .iter()
            .any(|f| f.suggested_fixes.iter().any(|s| s.contains("npm install"))));
    }

    // ─── Go errors ──────────────────────────────────────────────
    #[test]
    fn test_go_undefined_error() {
        let output = "./main.go:15:5: undefined: handleRequest";
        let failures = FailureAnalyzer::analyze(output, 1);
        assert!(failures
            .iter()
            .any(|f| f.failure_type == FailureType::SyntaxError));
        assert_eq!(failures[0].file_path.as_deref(), Some("./main.go"));
        assert_eq!(failures[0].line_number, Some(15));
    }

    #[test]
    fn test_go_missing_package() {
        let output = "cannot find package 'github.com/gin-gonic/gin' in any of:";
        let failures = FailureAnalyzer::analyze(output, 1);
        assert!(failures
            .iter()
            .any(|f| f.failure_type == FailureType::DependencyMissing));
    }

    // ─── Test failures ──────────────────────────────────────────
    #[test]
    fn test_cargo_test_failure() {
        let output = "test result: FAILED. 3 passed; 1 failed; 0 ignored";
        let failures = FailureAnalyzer::analyze(output, 1);
        assert!(failures
            .iter()
            .any(|f| f.failure_type == FailureType::TestFailure));
    }

    // ─── Successful command returns no failures ─────────────────
    #[test]
    fn test_success_no_failures() {
        let failures = FailureAnalyzer::analyze("all good", 0);
        assert!(failures.is_empty());
    }

    // ─── Unknown fallback ───────────────────────────────────────
    #[test]
    fn test_unknown_error_fallback() {
        let output = "some weird error nobody expected";
        let failures = FailureAnalyzer::analyze(output, 1);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].failure_type, FailureType::Unknown);
    }

    struct JsonFixProvider;

    #[async_trait]
    impl LlmProvider for JsonFixProvider {
        async fn complete(
            &self,
            _messages: Vec<crate::provider::LlmMessage>,
            _tools: Option<Vec<ToolDefinition>>,
            _model_override: Option<String>,
        ) -> Result<LlmResponse, ProviderError> {
            Ok(LlmResponse {
                content: Some(
                    r#"{"suggested_fixes":["cargo fmt --all","cargo check --workspace"]}"#
                        .to_string(),
                ),
                tool_calls: Vec::new(),
                finish_reason: FinishReason::Stop,
                usage: None,
            })
        }

        fn name(&self) -> &str {
            "json-fix-provider"
        }
    }

    #[tokio::test]
    async fn test_llm_suggest_fixes_parses_json_payload() {
        let toolkit =
            HealerToolkit::new(PathBuf::from("."), Some(Arc::new(JsonFixProvider)));
        let failure = Failure {
            failure_type: FailureType::Unknown,
            file_path: None,
            line_number: None,
            error_message: "unknown".to_string(),
            suggested_fixes: Vec::new(),
        };

        let fixes = toolkit
            .llm_suggest_fixes("cargo test", &failure, "test output")
            .await;
        assert_eq!(fixes.len(), 2);
        assert_eq!(fixes[0], "cargo fmt --all");
        assert_eq!(fixes[1], "cargo check --workspace");
    }
}
