//! Self-Healing Agent — Ported from Python healer.py
//!
//! Detects failures, generates fixes, applies them in sandbox,
//! and validates before suggesting. All operations go through
//! the security pipeline (PolicyEngine + Sandbox).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use tracing::{debug, info, warn};

use ownstack_engine::{
    PolicyDecision, PolicyEngine,
    ProcessSandbox, Sandbox, SandboxLevel,
};

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

        // Import errors
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

        // Syntax errors
        if output.contains("SyntaxError:") || output.contains("IndentationError:") {
            failures.push(Failure {
                failure_type: FailureType::SyntaxError,
                file_path: Self::extract_file_path(output),
                line_number: Self::extract_line_number(output),
                error_message: Self::extract_after(output, "SyntaxError:")
                    .unwrap_or_else(|| Self::extract_after(output, "IndentationError:").unwrap_or_default()),
                suggested_fixes: vec!["# Syntax error requires code review".to_string()],
            });
        }

        // Type errors
        if output.contains("TypeError:") || output.contains("AttributeError:") {
            failures.push(Failure {
                failure_type: FailureType::TypeError,
                file_path: Self::extract_file_path(output),
                line_number: Self::extract_line_number(output),
                error_message: Self::extract_after(output, "TypeError:")
                    .unwrap_or_else(|| Self::extract_after(output, "AttributeError:").unwrap_or_default()),
                suggested_fixes: Vec::new(),
            });
        }

        // Test failures
        if output.contains("FAILED") || output.contains("failed") {
            failures.push(Failure {
                failure_type: FailureType::TestFailure,
                file_path: Self::extract_file_path(output),
                line_number: Self::extract_line_number(output),
                error_message: "Test failure detected".to_string(),
                suggested_fixes: vec!["# Test failure requires LLM analysis".to_string()],
            });
        }

        // Dependency missing
        if output.contains("pip install") || output.contains("npm install") ||
           output.contains("Could not find a version") {
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
            let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            return num.parse().ok();
        }
        None
    }

    fn extract_install_commands(output: &str) -> Vec<String> {
        let mut cmds = Vec::new();
        for line in output.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("pip install") || trimmed.starts_with("npm install") {
                cmds.push(trimmed.to_string());
            }
        }
        cmds
    }
}

// ─── Self-Healing Engine ───────────────────────────────────────────

pub struct HealerToolkit {
    workspace: PathBuf,
}

impl HealerToolkit {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }

    /// Run the self-healing loop for a failing command
    pub fn heal(&self, command: &str, max_attempts: u32) -> HealingSession {
        let session_id = format!("heal-{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis());

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
        let initial = sandbox.exec(command, &self.workspace, SandboxLevel::Standard);
        session.original_output = format!("{}\n{}", initial.stdout, initial.stderr);

        if initial.success {
            session.healed = true;
            return session;
        }

        // Analyze failures
        session.failures_detected = FailureAnalyzer::analyze(
            &session.original_output, 1
        );

        info!("Healer: {} failures detected for: {}", session.failures_detected.len(), command);

        // Healing loop
        let mut applied_fixes: HashSet<String> = HashSet::new();

        for _ in 0..max_attempts {
            session.total_attempts += 1;

            if session.failures_detected.is_empty() {
                break;
            }

            let failure = session.failures_detected[0].clone();
            let fix = failure.suggested_fixes.iter()
                .find(|f| !applied_fixes.contains(*f))
                .cloned();

            let fix = match fix {
                Some(f) => f,
                None => {
                    info!("Healer: no new fixes available");
                    break;
                }
            };

            // Check policy for the fix command
            let decision = PolicyEngine::evaluate(&fix);
            if decision == PolicyDecision::Blocked {
                warn!("Healer: fix blocked by policy: {}", fix);
                break;
            }

            applied_fixes.insert(fix.clone());
            let start = std::time::Instant::now();

            // Apply fix
            let fix_result = sandbox.exec(&fix, &self.workspace, SandboxLevel::Standard);
            debug!("Healer: applied fix '{}' → success={}", fix, fix_result.success);

            // Re-run original command
            let verify = sandbox.exec(command, &self.workspace, SandboxLevel::Standard);
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

fn default_max_attempts() -> u32 { 5 }

#[async_trait]
impl Toolkit for HealerToolkit {
    fn name(&self) -> &str {
        "healer"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![ToolDef {
            name: "heal".to_string(),
            description: "Run a command and automatically detect + fix failures".to_string(),
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
                let session = self.heal(&parsed.command, parsed.max_attempts);
                let summary = serde_json::to_string_pretty(&session)
                    .unwrap_or_else(|_| format!("healed: {}", session.healed));
                Ok(ToolResult::success(summary))
            }
            _ => Err(ToolkitError::ToolNotFound(tool_name.to_string())),
        }
    }
}
