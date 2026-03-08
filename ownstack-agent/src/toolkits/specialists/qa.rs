use crate::toolkits::{ToolDef, ToolResult, Toolkit, ToolkitError};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::info;

pub struct QAToolkit;

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

/// Execute a test runner and capture output.
fn run_test_command(
    runner: &str,
    filter: Option<&str>,
    cwd: &Path,
    timeout_secs: u64,
) -> (bool, String, String, u64) {
    let start = std::time::Instant::now();

    let mut cmd = match runner {
        "cargo_test" => {
            let mut c = Command::new("cargo");
            c.arg("test");
            if let Some(f) = filter {
                c.arg(f);
            }
            c.arg("--").arg("--nocapture");
            c
        }
        "pytest" => {
            let mut c = Command::new("python");
            c.args(["-m", "pytest", "-v"]);
            if let Some(f) = filter {
                c.arg("-k").arg(f);
            }
            c
        }
        "npm_test" => {
            let mut c = Command::new("npm");
            c.arg("test");
            c
        }
        "go_test" => {
            let mut c = Command::new("go");
            c.args(["test", "-v", "./..."]);
            if let Some(f) = filter {
                c.arg("-run").arg(f);
            }
            c
        }
        _ => {
            return (
                false,
                String::new(),
                format!("Unknown test runner: {runner}"),
                0,
            );
        }
    };

    cmd.current_dir(cwd);

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            return (
                false,
                String::new(),
                format!("Failed to execute {runner}: {e}"),
                start.elapsed().as_millis() as u64,
            );
        }
    };
    let _ = timeout_secs; // timeout handled by ProcessSandbox in production

    let duration_ms = start.elapsed().as_millis() as u64;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let success = output.status.success();

    (success, stdout, stderr, duration_ms)
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
                            "description": "Timeout in seconds (default: 120)."
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

                let cwd = std::env::current_dir().map_err(|e| {
                    ToolkitError::ExecutionFailed(format!(
                        "Failed to resolve current directory: {}",
                        e
                    ))
                })?;

                let provided_path = Path::new(&parsed.test_file);
                let resolved_path = if provided_path.is_absolute() {
                    provided_path.to_path_buf()
                } else {
                    cwd.join(provided_path)
                };
                let file_exists = resolved_path.exists();

                let analysis = analyze_failure(&parsed.error_output);
                let response = json!({
                    "test_file": normalize_path(provided_path),
                    "resolved_test_file": normalize_path(&resolved_path),
                    "test_file_exists": file_exists,
                    "category": analysis.category,
                    "signals": analysis.signals,
                    "suggestions": analysis.suggestions,
                });

                Ok(ToolResult::success(response.to_string()))
            }
            "run_tests" => {
                let parsed: RunTestsArgs =
                    serde_json::from_value(args).unwrap_or(RunTestsArgs {
                        runner: None,
                        filter: None,
                        path: None,
                        timeout_secs: None,
                    });

                let cwd = std::env::current_dir().map_err(|e| {
                    ToolkitError::ExecutionFailed(format!("cwd: {e}"))
                })?;
                let work_dir = if let Some(p) = parsed.path.as_deref() {
                    let candidate = Path::new(p);
                    if candidate.is_absolute() {
                        candidate.to_path_buf()
                    } else {
                        cwd.join(candidate)
                    }
                } else {
                    cwd
                };

                let runner = parsed
                    .runner
                    .as_deref()
                    .unwrap_or_else(|| detect_test_runner(&work_dir));
                let timeout = parsed.timeout_secs.unwrap_or(120);

                info!("QAToolkit: running tests with '{runner}' in {:?}", work_dir);

                let (success, stdout, stderr, duration_ms) =
                    run_test_command(runner, parsed.filter.as_deref(), &work_dir, timeout);

                // If tests failed, auto-analyze the output
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
                    format!("{}...[truncated, {} chars total]", &stdout[..20_000], stdout.len())
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
                    "stdout": stdout_trunc,
                    "stderr": stderr_trunc,
                    "failure_analysis": analysis,
                });

                if success {
                    Ok(ToolResult::success(response.to_string()))
                } else {
                    Ok(ToolResult::failure(response.to_string(), Some(1)))
                }
            }
            "list_test_files" => {
                let parsed: ListTestFilesArgs =
                    serde_json::from_value(args).unwrap_or_default();
                let cwd = std::env::current_dir().map_err(|e| {
                    ToolkitError::ExecutionFailed(format!(
                        "Failed to resolve current directory: {}",
                        e
                    ))
                })?;

                let root = if let Some(path) = parsed.path.as_deref() {
                    let p = Path::new(path);
                    if p.is_absolute() {
                        p.to_path_buf()
                    } else {
                        cwd.join(p)
                    }
                } else {
                    cwd.clone()
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
}
