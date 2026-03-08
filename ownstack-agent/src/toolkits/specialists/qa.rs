use crate::toolkits::{ToolDef, ToolResult, Toolkit, ToolkitError};
use async_trait::async_trait;
use ownstack_engine::{
    AuditEntry, AuditLogger, PathValidator, PolicyDecision, PolicyEngine,
    ProcessSandbox, Sandbox, SandboxLevel,
};
use serde::Deserialize;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tracing::info;

pub struct QAToolkit {
    workspace: PathBuf,
    path_validator: PathValidator,
    audit_logger: AuditLogger,
    session_id: String,
}

impl QAToolkit {
    pub fn new(workspace: PathBuf) -> Self {
        let session_id = format!(
            "qa-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );
        let path_validator = PathValidator::new(workspace.clone());
        let audit_logger = AuditLogger::new(workspace.clone());
        Self {
            workspace,
            path_validator,
            audit_logger,
            session_id,
        }
    }

    fn audit(
        &self,
        command: &str,
        policy_decision: PolicyDecision,
        success: bool,
        duration_ms: u64,
        paths_accessed: Vec<String>,
    ) {
        let entry = AuditEntry {
            timestamp: String::new(),
            session_id: self.session_id.clone(),
            action: "exec".to_string(),
            command: command.to_string(),
            policy_decision,
            tool_name: "qa.run_tests".to_string(),
            success,
            duration_ms,
            workspace: self.workspace.to_string_lossy().to_string(),
            paths_accessed,
        };

        if let Err(err) = self.audit_logger.log(entry) {
            tracing::warn!("qa audit log failed: {}", err);
        }
    }
}

impl Default for QAToolkit {
    fn default() -> Self {
        let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::new(workspace)
    }
}

#[derive(Deserialize)]
struct RunTestsArgs {
    runner: Option<String>,
    filter: Option<String>,
    path: Option<String>,
    timeout_secs: Option<u64>,
}

/// Detect the appropriate test runner from workspace files.
fn detect_test_runner(root: &Path) -> &'static str {
    if root.join("Cargo.toml").exists() {
        "cargo_test"
    } else if root.join("pytest.ini").exists()
        || root.join("setup.py").exists()
        || root.join("pyproject.toml").exists()
    {
        "pytest"
    } else if root.join("package.json").exists() {
        "npm_test"
    } else if root.join("go.mod").exists() {
        "go_test"
    } else {
        "unknown"
    }
}

fn quote_shell_arg(raw: &str) -> String {
    if raw.is_empty() {
        return "\"\"".to_string();
    }
    let needs_quotes = raw
        .chars()
        .any(|c| c.is_whitespace() || matches!(c, '"' | '\\' | '\''));
    if !needs_quotes {
        return raw.to_string();
    }

    let escaped = raw.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{}\"", escaped)
}

fn build_test_command(
    runner: &str,
    filter: Option<&str>,
) -> Result<String, ToolkitError> {
    let command = match runner {
        "cargo_test" => {
            let mut cmd = String::from("cargo test");
            if let Some(f) = filter {
                cmd.push(' ');
                cmd.push_str(&quote_shell_arg(f));
            }
            cmd.push_str(" -- --nocapture");
            cmd
        }
        "pytest" => {
            let mut cmd = String::from("python -m pytest -v");
            if let Some(f) = filter {
                cmd.push_str(" -k ");
                cmd.push_str(&quote_shell_arg(f));
            }
            cmd
        }
        "npm_test" => "npm test".to_string(),
        "go_test" => {
            let mut cmd = String::from("go test -v ./...");
            if let Some(f) = filter {
                cmd.push_str(" -run ");
                cmd.push_str(&quote_shell_arg(f));
            }
            cmd
        }
        _ => {
            return Err(ToolkitError::InvalidArguments(format!(
                "Unknown test runner: {runner}"
            )));
        }
    };

    Ok(command)
}

fn timeout_to_level(timeout_secs: u64) -> SandboxLevel {
    if timeout_secs <= 60 {
        SandboxLevel::Light
    } else if timeout_secs <= 300 {
        SandboxLevel::Standard
    } else {
        SandboxLevel::Strict
    }
}

fn should_skip_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    matches!(
        name,
        ".git" | "target" | "node_modules" | ".venv" | "venv" | "__pycache__"
    )
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn is_test_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if lower.ends_with(".test.js")
        || lower.ends_with(".spec.js")
        || lower.ends_with(".test.ts")
        || lower.ends_with(".spec.ts")
    {
        return true;
    }

    match ext.as_str() {
        "rs" => {
            lower.ends_with("_test.rs")
                || lower.ends_with("_tests.rs")
                || path
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .is_some_and(|p| p == "tests")
        }
        "py" => lower.starts_with("test_") || lower.ends_with("_test.py"),
        "go" => lower.ends_with("_test.go"),
        _ => false,
    }
}

fn collect_test_files(
    root: &Path,
    limit: usize,
) -> Result<Vec<PathBuf>, ToolkitError> {
    if !root.exists() {
        return Err(ToolkitError::ExecutionFailed(format!(
            "Scan path does not exist: {}",
            root.display()
        )));
    }
    if !root.is_dir() {
        return Err(ToolkitError::ExecutionFailed(format!(
            "Scan path is not a directory: {}",
            root.display()
        )));
    }

    fn walk(
        dir: &Path,
        root: &Path,
        limit: usize,
        out: &mut Vec<PathBuf>,
    ) -> Result<(), ToolkitError> {
        if out.len() >= limit {
            return Ok(());
        }
        let entries = fs::read_dir(dir).map_err(|e| {
            ToolkitError::ExecutionFailed(format!(
                "Failed to read directory {}: {}",
                dir.display(),
                e
            ))
        })?;

        for entry in entries {
            if out.len() >= limit {
                break;
            }
            let entry = match entry {
                Ok(v) => v,
                Err(_) => continue,
            };
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(v) => v,
                Err(_) => continue,
            };

            if file_type.is_symlink() {
                continue;
            }

            if file_type.is_dir() {
                if should_skip_dir(&path) {
                    continue;
                }
                walk(&path, root, limit, out)?;
                continue;
            }

            if !file_type.is_file() || !is_test_file(&path) {
                continue;
            }

            let display = match path.strip_prefix(root) {
                Ok(relative) => relative.to_path_buf(),
                Err(_) => path.clone(),
            };
            out.push(display);
        }

        Ok(())
    }

    let mut files = Vec::new();
    walk(root, root, limit, &mut files)?;
    files.sort_by_key(|p| normalize_path(p));
    Ok(files)
}

#[derive(Deserialize)]
struct AnalyzeTestFailureArgs {
    test_file: String,
    error_output: String,
}

#[derive(Deserialize, Default)]
struct ListTestFilesArgs {
    path: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FailureAnalysis {
    category: &'static str,
    signals: Vec<&'static str>,
    suggestions: Vec<&'static str>,
}

fn analyze_failure(error_output: &str) -> FailureAnalysis {
    let lower = error_output.to_ascii_lowercase();

    let has_any = |keywords: &[&str]| keywords.iter().any(|k| lower.contains(k));

    if has_any(&["assertion failed", "assertionerror", "expected", "actual"]) {
        return FailureAnalysis {
            category: "assertion_mismatch",
            signals: vec!["assertion failed / expected vs actual mismatch"],
            suggestions: vec![
                "Inspect expected values in test fixtures and golden files.",
                "Log intermediate values around the assertion site.",
                "Re-run the isolated test to validate deterministic behavior.",
            ],
        };
    }

    if has_any(&[
        "cannot find",
        "undeclared",
        "mismatched types",
        "type mismatch",
        "compilation failed",
    ]) {
        return FailureAnalysis {
            category: "compile_or_type_error",
            signals: vec!["compiler/type system rejected current code"],
            suggestions: vec![
                "Check recent signature/type changes around the failing module.",
                "Align test imports and symbols with the current API surface.",
                "Run a targeted build/check for the crate/package.",
            ],
        };
    }

    if has_any(&[
        "no such file or directory",
        "filenotfound",
        "could not read",
        "path not found",
    ]) {
        return FailureAnalysis {
            category: "missing_fixture_or_path",
            signals: vec!["filesystem path/fixture not found at runtime"],
            suggestions: vec![
                "Verify fixture paths are workspace-relative and committed.",
                "Ensure the test setup creates required temp files/directories.",
                "Normalize path separators for cross-platform execution.",
            ],
        };
    }

    if has_any(&["timeout", "timed out", "deadline exceeded"]) {
        return FailureAnalysis {
            category: "timeout_or_flaky",
            signals: vec!["execution exceeded expected timing budget"],
            suggestions: vec![
                "Stabilize external dependencies and remove hidden sleeps/races.",
                "Increase timeout only after root-cause isolation.",
                "Capture per-step timings to identify slow operations.",
            ],
        };
    }

    if has_any(&[
        "permission denied",
        "access denied",
        "operation not permitted",
    ]) {
        return FailureAnalysis {
            category: "permission_error",
            signals: vec!["operation blocked by permissions/policy"],
            suggestions: vec![
                "Check sandbox/policy constraints for the failing command.",
                "Avoid privileged paths and use workspace-local outputs.",
                "Validate file modes and ownership in test setup.",
            ],
        };
    }

    if has_any(&["panic", "stack trace", "segmentation fault", "fatal error"]) {
        return FailureAnalysis {
            category: "runtime_crash",
            signals: vec!["process crashed during test execution"],
            suggestions: vec![
                "Inspect the first panic/exception frame, not downstream noise.",
                "Reduce the test to a minimal reproducer and bisect changes.",
                "Add guards around unchecked assumptions in runtime code paths.",
            ],
        };
    }

    FailureAnalysis {
        category: "unknown",
        signals: vec!["no strong failure pattern detected"],
        suggestions: vec![
            "Capture full stderr/stdout with verbose mode.",
            "Run the exact test in isolation to remove suite side effects.",
            "Compare against last known passing commit to narrow regression.",
        ],
    }
}

#[async_trait]
impl Toolkit for QAToolkit {
    fn name(&self) -> &str {
        "qa"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef {
                name: "analyze_test_failure".to_string(),
                description:
                    "Analyze test failure output and return categorized remediation suggestions."
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "test_file": {
                            "type": "string",
                            "description": "Path to the failing test file (relative or absolute)."
                        },
                        "error_output": {
                            "type": "string",
                            "description": "Captured stderr/stdout from the failing run."
                        },
                    },
                    "required": ["test_file", "error_output"],
                }),
            },
            ToolDef {
                name: "list_test_files".to_string(),
                description:
                    "List test files across Rust/Python/JS/TS/Go in the workspace."
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Optional root path to scan. Defaults to current workspace."
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of files to return (default: 100, max: 500)."
                        }
                    },
                }),
            },
            ToolDef {
                name: "run_tests".to_string(),
                description:
                    "Execute tests using the appropriate runner (cargo test, pytest, npm test, go test). Auto-detects runner from workspace."
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "runner": {
                            "type": "string",
                            "description": "Test runner override: 'cargo_test', 'pytest', 'npm_test', 'go_test'. Auto-detected if omitted.",
                            "enum": ["cargo_test", "pytest", "npm_test", "go_test"]
                        },
                        "filter": {
                            "type": "string",
                            "description": "Filter pattern for test names (e.g. 'test_auth' for pytest -k, or test name for cargo test)."
                        },
                        "path": {
                            "type": "string",
                            "description": "Working directory for test execution. Defaults to current workspace."
                        },
                        "timeout_secs": {
                            "type": "integer",
                            "description": "Timeout in seconds (default: 120, clamped to 30..600)."
                        }
                    },
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
            "analyze_test_failure" => {
                let parsed: AnalyzeTestFailureArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;

                let provided_path = Path::new(&parsed.test_file);
                let resolved_path = self.path_validator.validate(provided_path).ok();
                let file_exists = resolved_path.as_ref().is_some_and(|p| p.exists());

                let analysis = analyze_failure(&parsed.error_output);
                let response = json!({
                    "test_file": normalize_path(provided_path),
                    "resolved_test_file": resolved_path
                        .as_deref()
                        .map(normalize_path)
                        .unwrap_or_else(|| "(outside-workspace)".to_string()),
                    "test_file_exists": file_exists,
                    "category": analysis.category,
                    "signals": analysis.signals,
                    "suggestions": analysis.suggestions,
                });

                Ok(ToolResult::success(response.to_string()))
            }
            "run_tests" => {
                let parsed: RunTestsArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;

                let work_dir = if let Some(p) = parsed.path.as_deref() {
                    self.path_validator
                        .validate(Path::new(p))
                        .map_err(|e| ToolkitError::SecurityViolation(e.to_string()))?
                } else {
                    self.workspace.clone()
                };

                if !work_dir.exists() {
                    return Err(ToolkitError::ExecutionFailed(format!(
                        "Working directory does not exist: {}",
                        work_dir.display()
                    )));
                }
                if !work_dir.is_dir() {
                    return Err(ToolkitError::ExecutionFailed(format!(
                        "Working directory is not a directory: {}",
                        work_dir.display()
                    )));
                }

                let runner = parsed
                    .runner
                    .as_deref()
                    .unwrap_or_else(|| detect_test_runner(&work_dir));
                if runner == "unknown" {
                    return Err(ToolkitError::InvalidArguments(
                        "Could not auto-detect test runner; provide runner explicitly"
                            .to_string(),
                    ));
                }

                let timeout_secs = parsed.timeout_secs.unwrap_or(120).clamp(30, 600);
                let command =
                    build_test_command(runner, parsed.filter.as_deref())?;

                let decision = PolicyEngine::evaluate(&command);
                match decision {
                    PolicyDecision::Blocked => {
                        self.audit(
                            &command,
                            PolicyDecision::Blocked,
                            false,
                            0,
                            vec![work_dir.to_string_lossy().to_string()],
                        );
                        return Err(ToolkitError::SecurityViolation(format!(
                            "Command blocked by policy: {}",
                            command
                        )));
                    }
                    PolicyDecision::Ask => {
                        self.audit(
                            &command,
                            PolicyDecision::Ask,
                            false,
                            0,
                            vec![work_dir.to_string_lossy().to_string()],
                        );
                        return Err(ToolkitError::SecurityViolation(format!(
                            "Command requires approval: {}",
                            command
                        )));
                    }
                    PolicyDecision::Auto => {}
                }

                let sandbox = ProcessSandbox;
                let level = timeout_to_level(timeout_secs);
                let started = Instant::now();
                info!(
                    "QAToolkit: running tests with '{}' in {:?}",
                    runner, work_dir
                );

                let sandbox_result = sandbox.exec(&command, &work_dir, level).await;
                let duration_ms = started.elapsed().as_millis() as u64;
                self.audit(
                    &command,
                    PolicyDecision::Auto,
                    sandbox_result.success,
                    duration_ms,
                    vec![work_dir.to_string_lossy().to_string()],
                );

                let stdout = sandbox_result.stdout;
                let stderr = sandbox_result.stderr;
                let success = sandbox_result.success;

                let analysis = if !success {
                    let failure = analyze_failure(&format!("{stdout}\n{stderr}"));
                    Some(json!({
                        "category": failure.category,
                        "signals": failure.signals,
                        "suggestions": failure.suggestions,
                    }))
                } else {
                    None
                };

                // Truncate very long output
                let stdout_trunc = if stdout.len() > 20_000 {
                    format!(
                        "{}...[truncated, {} chars total]",
                        &stdout[..20_000],
                        stdout.len()
                    )
                } else {
                    stdout
                };
                let stderr_trunc = if stderr.len() > 10_000 {
                    format!("{}...[truncated]", &stderr[..10_000])
                } else {
                    stderr
                };

                let response = json!({
                    "runner": runner,
                    "success": success,
                    "duration_ms": duration_ms,
                    "requested_timeout_secs": timeout_secs,
                    "sandbox_level": format!("{:?}", level),
                    "command": command,
                    "work_dir": normalize_path(&work_dir),
                    "stdout": stdout_trunc,
                    "stderr": stderr_trunc,
                    "failure_analysis": analysis,
                });

                if success {
                    Ok(ToolResult::success(response.to_string()))
                } else {
                    Ok(ToolResult::failure(
                        response.to_string(),
                        sandbox_result.exit_code.or(Some(1)),
                    ))
                }
            }
            "list_test_files" => {
                let parsed: ListTestFilesArgs = serde_json::from_value(args)
                    .unwrap_or_default();

                let root = if let Some(path) = parsed.path.as_deref() {
                    self.path_validator
                        .validate(Path::new(path))
                        .map_err(|e| ToolkitError::SecurityViolation(e.to_string()))?
                } else {
                    self.workspace.clone()
                };

                let limit = parsed.limit.unwrap_or(100).clamp(1, 500);
                let files = collect_test_files(&root, limit)?;
                let serialized_files =
                    files.iter().map(|p| normalize_path(p)).collect::<Vec<_>>();

                let response = json!({
                    "root": normalize_path(&root),
                    "count": serialized_files.len(),
                    "limit": limit,
                    "files": serialized_files,
                });
                Ok(ToolResult::success(response.to_string()))
            }
            _ => Err(ToolkitError::ToolNotFound(tool_name.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolkits::Toolkit;

    #[test]
    fn detects_common_test_file_patterns() {
        assert!(is_test_file(Path::new("tests/auth_flow.rs")));
        assert!(is_test_file(Path::new("src/login_test.rs")));
        assert!(is_test_file(Path::new("tests/test_api.py")));
        assert!(is_test_file(Path::new("web/login.spec.ts")));
        assert!(is_test_file(Path::new("backend/worker_test.go")));
        assert!(!is_test_file(Path::new("src/main.rs")));
    }

    #[test]
    fn classifies_assertion_error() {
        let analysis = analyze_failure("AssertionError: expected 2 got 3");
        assert_eq!(analysis.category, "assertion_mismatch");
        assert!(!analysis.suggestions.is_empty());
    }

    #[test]
    fn classifies_timeout_error() {
        let analysis = analyze_failure("test timed out after 30s");
        assert_eq!(analysis.category, "timeout_or_flaky");
    }

    #[test]
    fn collects_test_files_from_workspace() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        fs::create_dir_all(root.join("tests")).expect("mkdir tests");
        fs::create_dir_all(root.join("src")).expect("mkdir src");
        fs::write(root.join("tests").join("auth.rs"), "").expect("write");
        fs::write(root.join("src").join("user_test.rs"), "").expect("write");
        fs::write(root.join("src").join("main.rs"), "").expect("write");

        let files = collect_test_files(root, 10).expect("collect");
        let normalized = files.iter().map(|p| normalize_path(p)).collect::<Vec<_>>();

        assert!(normalized.iter().any(|p| p.ends_with("tests/auth.rs")));
        assert!(normalized.iter().any(|p| p.ends_with("src/user_test.rs")));
        assert!(!normalized.iter().any(|p| p.ends_with("src/main.rs")));
    }

    #[test]
    fn build_test_command_quotes_filter() {
        let cmd = build_test_command("pytest", Some("name with space")).expect("cmd");
        assert!(cmd.contains("-k \"name with space\""));
    }

    #[tokio::test]
    async fn run_tests_rejects_parent_traversal_path() {
        let temp = tempfile::tempdir().expect("tempdir");
        let toolkit = QAToolkit::new(temp.path().to_path_buf());

        let result = toolkit
            .execute(
                "run_tests",
                json!({
                    "runner": "cargo_test",
                    "path": "../outside"
                }),
            )
            .await;

        assert!(matches!(result, Err(ToolkitError::SecurityViolation(_))));
    }
}
