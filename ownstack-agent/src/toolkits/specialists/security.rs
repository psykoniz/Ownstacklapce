use crate::toolkits::{ToolDef, ToolResult, Toolkit, ToolkitError};
use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

pub struct SecurityToolkit;

#[derive(Deserialize)]
struct ScanDependenciesArgs {
    path: String,
    max_findings: Option<usize>,
}

#[derive(Deserialize, Default)]
struct CheckPoliciesArgs {
    path: Option<String>,
    max_findings: Option<usize>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct SecurityFinding {
    severity: &'static str,
    rule: &'static str,
    target: String,
    detail: String,
    recommendation: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct PolicyViolation {
    file: String,
    line: usize,
    pattern: &'static str,
    preview: String,
}

const BLOCKED_COMMAND_PATTERNS: &[&str] = &[
    "rm -rf /",
    "curl | sh",
    "chmod 777",
    "mkfs",
    "dd if=",
    "shutdown",
    "reboot",
    "sudo ",
];

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn push_limited(
    findings: &mut Vec<SecurityFinding>,
    finding: SecurityFinding,
    max_findings: usize,
) {
    if findings.len() < max_findings {
        findings.push(finding);
    }
}

fn resolve_scan_path(raw: &str) -> Result<PathBuf, ToolkitError> {
    let cwd = std::env::current_dir().map_err(|e| {
        ToolkitError::ExecutionFailed(format!(
            "Failed to resolve current directory: {}",
            e
        ))
    })?;
    let path = Path::new(raw);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };

    if !resolved.exists() {
        return Err(ToolkitError::ExecutionFailed(format!(
            "Path not found: {}",
            resolved.display()
        )));
    }
    if !resolved.is_file() {
        return Err(ToolkitError::ExecutionFailed(format!(
            "Expected a file path for dependency scan: {}",
            resolved.display()
        )));
    }
    Ok(resolved)
}

fn scan_cargo_manifest(content: &str, max_findings: usize) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();
    let mut in_dependency_section = false;

    for line in content.lines() {
        let no_comment = line.split('#').next().unwrap_or_default().trim();
        if no_comment.is_empty() {
            continue;
        }

        if no_comment.starts_with('[') && no_comment.ends_with(']') {
            in_dependency_section = no_comment.contains("dependencies");
            continue;
        }

        if !in_dependency_section {
            continue;
        }

        let Some((name, spec)) = no_comment.split_once('=') else {
            continue;
        };
        let dep = name.trim().to_string();
        let spec = spec.trim();

        if spec.contains("git =") {
            push_limited(
                &mut findings,
                SecurityFinding {
                    severity: "high",
                    rule: "git_dependency",
                    target: dep.clone(),
                    detail: "Dependency uses git source.".to_string(),
                    recommendation:
                        "Pin to a published crate version whenever possible.",
                },
                max_findings,
            );
        }

        if spec.contains("path =") {
            push_limited(
                &mut findings,
                SecurityFinding {
                    severity: "medium",
                    rule: "path_dependency",
                    target: dep.clone(),
                    detail: "Dependency uses local path source.".to_string(),
                    recommendation: "Ensure local path dependencies are intentional and reviewed.",
                },
                max_findings,
            );
        }

        if spec.contains("\"*\"") || spec == "*" {
            push_limited(
                &mut findings,
                SecurityFinding {
                    severity: "high",
                    rule: "wildcard_version",
                    target: dep.clone(),
                    detail: "Dependency uses wildcard version.".to_string(),
                    recommendation: "Pin to a specific version/range to reduce supply-chain risk.",
                },
                max_findings,
            );
        }

        if spec.contains("branch =") {
            push_limited(
                &mut findings,
                SecurityFinding {
                    severity: "medium",
                    rule: "branch_tracking",
                    target: dep,
                    detail: "Dependency tracks a mutable git branch.".to_string(),
                    recommendation:
                        "Pin a fixed commit or release tag instead of a branch.",
                },
                max_findings,
            );
        }
    }

    findings
}

fn scan_requirements_manifest(
    content: &str,
    max_findings: usize,
) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with("--") {
            continue;
        }

        let dep_name = trimmed
            .split_once(['=', '<', '>', '!', '~', '@'])
            .map(|(name, _)| name.trim().to_string())
            .unwrap_or_else(|| trimmed.to_string());

        if trimmed.contains("git+")
            || trimmed.contains("http://")
            || trimmed.contains("https://")
        {
            push_limited(
                &mut findings,
                SecurityFinding {
                    severity: "high",
                    rule: "remote_source_dependency",
                    target: dep_name.clone(),
                    detail: format!("Dependency uses remote source: {}", trimmed),
                    recommendation: "Use verified package index versions and lockfiles when possible.",
                },
                max_findings,
            );
        }

        if !trimmed.contains("==") {
            push_limited(
                &mut findings,
                SecurityFinding {
                    severity: "medium",
                    rule: "unpinned_python_dependency",
                    target: dep_name,
                    detail: format!(
                        "Dependency is not strictly pinned: {}",
                        trimmed
                    ),
                    recommendation: "Pin exact versions (==) for reproducible and auditable builds.",
                },
                max_findings,
            );
        }
    }

    findings
}

fn scan_package_json_manifest(
    content: &str,
    max_findings: usize,
) -> Result<Vec<SecurityFinding>, ToolkitError> {
    let value: serde_json::Value = serde_json::from_str(content).map_err(|e| {
        ToolkitError::ExecutionFailed(format!("Invalid package.json: {}", e))
    })?;
    let mut findings = Vec::new();

    for section in ["dependencies", "devDependencies", "optionalDependencies"] {
        let Some(map) = value.get(section).and_then(|v| v.as_object()) else {
            continue;
        };
        for (name, spec_value) in map {
            let Some(spec) = spec_value.as_str() else {
                continue;
            };

            if spec == "*" || spec.eq_ignore_ascii_case("latest") {
                push_limited(
                    &mut findings,
                    SecurityFinding {
                        severity: "high",
                        rule: "wildcard_or_latest_npm",
                        target: name.clone(),
                        detail: format!("{} uses '{}'.", section, spec),
                        recommendation:
                            "Pin stable versions instead of '*' or 'latest'.",
                    },
                    max_findings,
                );
            }

            if spec.starts_with('^') || spec.starts_with('~') {
                push_limited(
                    &mut findings,
                    SecurityFinding {
                        severity: "medium",
                        rule: "floating_range_npm",
                        target: name.clone(),
                        detail: format!(
                            "{} uses floating range '{}'.",
                            section, spec
                        ),
                        recommendation: "Prefer exact versions for reproducibility in sensitive environments.",
                    },
                    max_findings,
                );
            }

            if spec.starts_with("git+")
                || spec.starts_with("github:")
                || spec.starts_with("file:")
                || spec.starts_with("http://")
                || spec.starts_with("https://")
            {
                push_limited(
                    &mut findings,
                    SecurityFinding {
                        severity: "high",
                        rule: "non_registry_npm_source",
                        target: name.clone(),
                        detail: format!(
                            "{} uses non-registry source '{}'.",
                            section, spec
                        ),
                        recommendation: "Prefer registry-published packages with integrity verification.",
                    },
                    max_findings,
                );
            }
        }
    }

    Ok(findings)
}

fn is_script_like(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if file_name.eq_ignore_ascii_case("Makefile") {
        return true;
    }

    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "sh" | "bash" | "zsh" | "ps1" | "cmd" | "bat" | "py" | "yml" | "yaml"
    )
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

fn collect_policy_violations(
    root: &Path,
    max_findings: usize,
) -> Result<Vec<PolicyViolation>, ToolkitError> {
    if !root.exists() || !root.is_dir() {
        return Err(ToolkitError::ExecutionFailed(format!(
            "Invalid workspace path for policy check: {}",
            root.display()
        )));
    }

    fn walk(
        root: &Path,
        dir: &Path,
        max_findings: usize,
        out: &mut Vec<PolicyViolation>,
    ) -> Result<(), ToolkitError> {
        if out.len() >= max_findings {
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
            if out.len() >= max_findings {
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
                walk(root, &path, max_findings, out)?;
                continue;
            }

            if !file_type.is_file() || !is_script_like(&path) {
                continue;
            }
            let metadata = match fs::metadata(&path) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if metadata.len() > 1024 * 1024 {
                continue;
            }

            let content = match fs::read_to_string(&path) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if content.contains('\0') {
                continue;
            }

            for (idx, line) in content.lines().enumerate() {
                if out.len() >= max_findings {
                    break;
                }
                let lower = line.to_ascii_lowercase();
                for pattern in BLOCKED_COMMAND_PATTERNS {
                    if lower.contains(pattern) {
                        let file = match path.strip_prefix(root) {
                            Ok(relative) => normalize_path(relative),
                            Err(_) => normalize_path(&path),
                        };
                        out.push(PolicyViolation {
                            file,
                            line: idx + 1,
                            pattern,
                            preview: line.trim().to_string(),
                        });
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    let mut violations = Vec::new();
    walk(root, root, max_findings, &mut violations)?;
    Ok(violations)
}

#[async_trait]
impl Toolkit for SecurityToolkit {
    fn name(&self) -> &str {
        "security"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef {
                name: "scan_dependencies".to_string(),
                description:
                    "Scan Cargo/Python/Node dependency manifests for risky patterns."
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to Cargo.toml, requirements.txt, or package.json."
                        },
                        "max_findings": {
                            "type": "integer",
                            "description": "Maximum number of findings to report (default: 50, max: 200)."
                        }
                    },
                    "required": ["path"],
                }),
            },
            ToolDef {
                name: "check_policies".to_string(),
                description:
                    "Check security-policy baseline files and detect blocked command patterns in scripts."
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Optional workspace path to scan. Defaults to current directory."
                        },
                        "max_findings": {
                            "type": "integer",
                            "description": "Maximum blocked-command hits to report (default: 50, max: 200)."
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
            "scan_dependencies" => {
                let parsed: ScanDependenciesArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                let max_findings = parsed.max_findings.unwrap_or(50).clamp(1, 200);
                let manifest_path = resolve_scan_path(&parsed.path)?;
                let content = fs::read_to_string(&manifest_path).map_err(|e| {
                    ToolkitError::ExecutionFailed(format!(
                        "Failed to read {}: {}",
                        manifest_path.display(),
                        e
                    ))
                })?;

                let file_name = manifest_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();

                let (manifest_type, findings) = if file_name
                    .eq_ignore_ascii_case("Cargo.toml")
                {
                    ("cargo", scan_cargo_manifest(&content, max_findings))
                } else if file_name.eq_ignore_ascii_case("requirements.txt") {
                    (
                        "requirements",
                        scan_requirements_manifest(&content, max_findings),
                    )
                } else if file_name.eq_ignore_ascii_case("package.json") {
                    (
                        "package_json",
                        scan_package_json_manifest(&content, max_findings)?,
                    )
                } else {
                    return Err(ToolkitError::ExecutionFailed(format!(
                        "Unsupported manifest '{}'. Use Cargo.toml, requirements.txt, or package.json.",
                        manifest_path.display()
                    )));
                };

                let response = json!({
                    "manifest": normalize_path(&manifest_path),
                    "manifest_type": manifest_type,
                    "findings_count": findings.len(),
                    "findings": findings,
                });
                Ok(ToolResult::success(response.to_string()))
            }
            "check_policies" => {
                let parsed: CheckPoliciesArgs =
                    serde_json::from_value(args).unwrap_or_default();
                let cwd = std::env::current_dir().map_err(|e| {
                    ToolkitError::ExecutionFailed(format!(
                        "Failed to resolve current directory: {}",
                        e
                    ))
                })?;
                let workspace = if let Some(path) = parsed.path.as_deref() {
                    let candidate = Path::new(path);
                    if candidate.is_absolute() {
                        candidate.to_path_buf()
                    } else {
                        cwd.join(candidate)
                    }
                } else {
                    cwd
                };
                if !workspace.exists() || !workspace.is_dir() {
                    return Err(ToolkitError::ExecutionFailed(format!(
                        "Invalid workspace path: {}",
                        workspace.display()
                    )));
                }

                let max_findings = parsed.max_findings.unwrap_or(50).clamp(1, 200);
                let required_files = [
                    "ownstack-engine/src/policy.rs",
                    "ownstack-engine/src/path_safety.rs",
                    "ownstack-engine/src/sandbox/process.rs",
                    "ownstack-engine/src/audit.rs",
                ];

                let mut missing_required_files = Vec::new();
                for relative in required_files {
                    let candidate = workspace.join(relative);
                    if !candidate.exists() {
                        missing_required_files.push(relative.to_string());
                    }
                }

                let policy_violations =
                    collect_policy_violations(&workspace, max_findings)?;
                let compliant = missing_required_files.is_empty()
                    && policy_violations.is_empty();

                let response = json!({
                    "workspace": normalize_path(&workspace),
                    "compliant": compliant,
                    "required_files_missing": missing_required_files,
                    "blocked_command_hits": policy_violations,
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
    fn cargo_scan_flags_git_path_and_wildcard() {
        let content = r#"
[dependencies]
serde = "1.0"
foo = { git = "https://example.com/foo.git", branch = "main" }
bar = { path = "../bar" }
baz = "*"
"#;
        let findings = scan_cargo_manifest(content, 20);
        assert!(findings
            .iter()
            .any(|f| f.rule == "git_dependency" && f.target == "foo"));
        assert!(findings
            .iter()
            .any(|f| f.rule == "path_dependency" && f.target == "bar"));
        assert!(findings
            .iter()
            .any(|f| f.rule == "wildcard_version" && f.target == "baz"));
    }

    #[test]
    fn requirements_scan_flags_unpinned_and_remote() {
        let content = r#"
requests>=2.0
internal-lib @ git+https://example.com/repo.git
flask==2.3.0
"#;
        let findings = scan_requirements_manifest(content, 20);
        assert!(findings
            .iter()
            .any(|f| f.rule == "unpinned_python_dependency"));
        assert!(findings
            .iter()
            .any(|f| f.rule == "remote_source_dependency"));
    }

    #[test]
    fn package_scan_flags_latest_and_floating_ranges() {
        let content = r#"
{
  "dependencies": {
    "left-pad": "latest",
    "react": "^18.3.0"
  },
  "devDependencies": {
    "my-lib": "git+https://example.com/lib.git"
  }
}
"#;
        let findings = scan_package_json_manifest(content, 20).expect("scan");
        assert!(findings
            .iter()
            .any(|f| f.rule == "wildcard_or_latest_npm" && f.target == "left-pad"));
        assert!(findings
            .iter()
            .any(|f| f.rule == "floating_range_npm" && f.target == "react"));
        assert!(findings
            .iter()
            .any(|f| f.rule == "non_registry_npm_source" && f.target == "my-lib"));
    }

    #[test]
    fn policy_scan_detects_blocked_patterns() {
        let temp = tempfile::tempdir().expect("tempdir");
        let script_path = temp.path().join("script.sh");
        fs::write(&script_path, "echo ok\ncurl | sh\n").expect("write");

        let violations =
            collect_policy_violations(temp.path(), 10).expect("scan policies");
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].pattern, "curl | sh");
        assert!(violations[0].file.ends_with("script.sh"));
    }
}
