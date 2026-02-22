use crate::toolkits::{ToolDef, ToolResult, Toolkit, ToolkitError};
use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

pub struct ReviewerToolkit;

#[derive(Deserialize)]
struct AnalyzeComplexityArgs {
    file_path: String,
}

#[derive(Deserialize)]
struct CheckStyleComplianceArgs {
    file_path: String,
    max_line_length: Option<usize>,
    max_findings: Option<usize>,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
struct ComplexityMetrics {
    file: String,
    line_count: usize,
    non_empty_lines: usize,
    function_count: usize,
    branch_points: usize,
    cyclomatic_estimate: usize,
    max_nesting_depth: usize,
    maintainability_index: f64,
    risk: &'static str,
    recommendations: Vec<&'static str>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct StyleViolation {
    line: usize,
    rule: &'static str,
    preview: String,
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn resolve_file_path(file_path: &str) -> Result<PathBuf, ToolkitError> {
    let cwd = std::env::current_dir().map_err(|e| {
        ToolkitError::ExecutionFailed(format!(
            "Failed to resolve current directory: {}",
            e
        ))
    })?;
    let requested = Path::new(file_path);
    let resolved = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        cwd.join(requested)
    };

    if !resolved.exists() {
        return Err(ToolkitError::ExecutionFailed(format!(
            "File not found: {}",
            resolved.display()
        )));
    }
    if !resolved.is_file() {
        return Err(ToolkitError::ExecutionFailed(format!(
            "Path is not a file: {}",
            resolved.display()
        )));
    }
    Ok(resolved)
}

fn read_text_file(path: &Path) -> Result<String, ToolkitError> {
    let metadata = fs::metadata(path).map_err(|e| {
        ToolkitError::ExecutionFailed(format!(
            "Failed to read metadata {}: {}",
            path.display(),
            e
        ))
    })?;
    if metadata.len() > 2 * 1024 * 1024 {
        return Err(ToolkitError::ExecutionFailed(format!(
            "File too large for analysis (>{} bytes): {}",
            2 * 1024 * 1024,
            path.display()
        )));
    }

    let content = fs::read_to_string(path).map_err(|e| {
        ToolkitError::ExecutionFailed(format!(
            "Failed to read file {}: {}",
            path.display(),
            e
        ))
    })?;
    if content.contains('\0') {
        return Err(ToolkitError::ExecutionFailed(format!(
            "Binary file detected, expected text: {}",
            path.display()
        )));
    }
    Ok(content)
}

fn count_function_definitions(line: &str) -> usize {
    let trimmed = line.trim_start();
    let lowered = trimmed.to_ascii_lowercase();
    if lowered.starts_with("fn ")
        || lowered.starts_with("pub fn ")
        || lowered.starts_with("async fn ")
        || lowered.starts_with("pub async fn ")
    {
        return 1;
    }
    if lowered.starts_with("def ")
        || lowered.starts_with("async def ")
        || lowered.starts_with("function ")
        || lowered.starts_with("func ")
    {
        return 1;
    }
    0
}

fn count_branch_points(line: &str) -> usize {
    let lowered = line.to_ascii_lowercase();
    let mut count = 0;
    for keyword in [
        " if ",
        " else if ",
        " match ",
        " case ",
        " switch ",
        " for ",
        " while ",
        " catch ",
    ] {
        if lowered.contains(keyword) {
            count += 1;
        }
    }
    count += lowered.matches("&&").count();
    count += lowered.matches("||").count();
    count += lowered.matches('?').count();
    count
}

fn estimate_max_nesting_depth(content: &str) -> usize {
    let mut depth = 0usize;
    let mut max_depth = 0usize;

    for line in content.lines() {
        let mut in_string = false;
        for ch in line.chars() {
            match ch {
                '"' => in_string = !in_string,
                '{' if !in_string => {
                    depth += 1;
                    if depth > max_depth {
                        max_depth = depth;
                    }
                }
                '}' if !in_string => {
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
        }
    }
    max_depth
}

fn maintainability_index(
    line_count: usize,
    cyclomatic_estimate: usize,
    max_nesting_depth: usize,
) -> f64 {
    let score = 100.0
        - (cyclomatic_estimate as f64 * 1.6)
        - (max_nesting_depth as f64 * 3.0)
        - (line_count as f64 / 18.0);
    score.clamp(0.0, 100.0)
}

fn complexity_risk(cyclomatic_estimate: usize) -> &'static str {
    if cyclomatic_estimate <= 10 {
        "low"
    } else if cyclomatic_estimate <= 20 {
        "medium"
    } else if cyclomatic_estimate <= 35 {
        "high"
    } else {
        "critical"
    }
}

fn complexity_recommendations(
    cyclomatic_estimate: usize,
    max_nesting_depth: usize,
    line_count: usize,
) -> Vec<&'static str> {
    let mut recs = Vec::new();
    if cyclomatic_estimate > 20 {
        recs.push("Split decision-heavy logic into smaller dedicated helpers.");
    }
    if max_nesting_depth > 4 {
        recs.push("Flatten nested branches using guard clauses/early returns.");
    }
    if line_count > 350 {
        recs.push("Consider splitting this file/module into focused units.");
    }
    if recs.is_empty() {
        recs.push("Complexity is acceptable for current thresholds.");
    }
    recs
}

fn analyze_complexity(path: &Path, content: &str) -> ComplexityMetrics {
    let line_count = content.lines().count();
    let non_empty_lines = content.lines().filter(|l| !l.trim().is_empty()).count();

    let function_count = content
        .lines()
        .map(count_function_definitions)
        .sum::<usize>();
    let branch_points = content.lines().map(count_branch_points).sum::<usize>();
    let cyclomatic_estimate = 1 + branch_points;
    let max_nesting_depth = estimate_max_nesting_depth(content);
    let maintainability_index =
        maintainability_index(line_count, cyclomatic_estimate, max_nesting_depth);
    let risk = complexity_risk(cyclomatic_estimate);
    let recommendations = complexity_recommendations(
        cyclomatic_estimate,
        max_nesting_depth,
        line_count,
    );

    ComplexityMetrics {
        file: normalize_path(path),
        line_count,
        non_empty_lines,
        function_count,
        branch_points,
        cyclomatic_estimate,
        max_nesting_depth,
        maintainability_index,
        risk,
        recommendations,
    }
}

fn check_style(
    content: &str,
    max_line_length: usize,
    max_findings: usize,
) -> Vec<StyleViolation> {
    let mut findings = Vec::new();

    for (idx, line) in content.lines().enumerate() {
        if findings.len() >= max_findings {
            break;
        }
        let line_no = idx + 1;
        let trimmed_end = line.trim_end();

        if trimmed_end.len() != line.len() {
            findings.push(StyleViolation {
                line: line_no,
                rule: "trailing_whitespace",
                preview: line.to_string(),
            });
            continue;
        }

        if line.chars().count() > max_line_length {
            findings.push(StyleViolation {
                line: line_no,
                rule: "line_too_long",
                preview: line.to_string(),
            });
            continue;
        }

        if line.contains('\t') {
            findings.push(StyleViolation {
                line: line_no,
                rule: "tab_indentation",
                preview: line.to_string(),
            });
            continue;
        }

        let lower = line.to_ascii_lowercase();
        for (pattern, rule) in [
            ("unwrap()", "forbidden_unwrap"),
            ("println!(", "forbidden_println"),
            ("dbg!(", "debug_macro_leftover"),
            ("todo!(", "todo_leftover"),
            ("fixme", "fixme_marker"),
        ] {
            if lower.contains(pattern) {
                findings.push(StyleViolation {
                    line: line_no,
                    rule,
                    preview: line.to_string(),
                });
                break;
            }
        }
    }

    findings
}

#[async_trait]
impl Toolkit for ReviewerToolkit {
    fn name(&self) -> &str {
        "reviewer"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef {
                name: "analyze_complexity".to_string(),
                description: "Estimate code complexity and maintainability from a source file."
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "file_path": {
                            "type": "string",
                            "description": "Path to the source file to analyze."
                        },
                    },
                    "required": ["file_path"],
                }),
            },
            ToolDef {
                name: "check_style_compliance".to_string(),
                description:
                    "Check style compliance (line length, trailing whitespace, forbidden patterns)."
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "file_path": {
                            "type": "string",
                            "description": "Path to the file to check."
                        },
                        "max_line_length": {
                            "type": "integer",
                            "description": "Maximum accepted line length (default 120)."
                        },
                        "max_findings": {
                            "type": "integer",
                            "description": "Maximum violations to return (default 200, max 1000)."
                        }
                    },
                    "required": ["file_path"],
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
            "analyze_complexity" => {
                let parsed: AnalyzeComplexityArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                let path = resolve_file_path(&parsed.file_path)?;
                let content = read_text_file(&path)?;
                let metrics = analyze_complexity(&path, &content);
                Ok(ToolResult::success(json!(metrics).to_string()))
            }
            "check_style_compliance" => {
                let parsed: CheckStyleComplianceArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                let path = resolve_file_path(&parsed.file_path)?;
                let content = read_text_file(&path)?;
                let max_line_length =
                    parsed.max_line_length.unwrap_or(120).clamp(40, 400);
                let max_findings = parsed.max_findings.unwrap_or(200).clamp(1, 1000);

                let violations =
                    check_style(&content, max_line_length, max_findings);
                let response = json!({
                    "file": normalize_path(&path),
                    "compliant": violations.is_empty(),
                    "violations_count": violations.len(),
                    "violations": violations,
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
    fn detects_branch_complexity() {
        let source = r#"
pub fn process(x: i32) -> i32 {
    if x > 0 {
        for i in 0..x {
            if i % 2 == 0 { }
        }
    } else if x < 0 {
        while x < 0 { break; }
    }
    0
}
"#;
        let metrics = analyze_complexity(Path::new("sample.rs"), source);
        assert!(metrics.cyclomatic_estimate >= 5);
        assert!(metrics.function_count >= 1);
    }

    #[test]
    fn style_checker_detects_common_violations() {
        let source = "let x = 1;   \nprintln!(\"x\");\n\tlet y = 2;\n";
        let violations = check_style(source, 120, 10);
        assert!(violations.iter().any(|v| v.rule == "trailing_whitespace"));
        assert!(violations.iter().any(|v| v.rule == "forbidden_println"));
        assert!(violations.iter().any(|v| v.rule == "tab_indentation"));
    }

    #[test]
    fn resolves_relative_file_path() {
        let temp = tempfile::tempdir().expect("tempdir");
        let file = temp.path().join("a.rs");
        fs::write(&file, "fn main() {}").expect("write file");

        let original_cwd = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(temp.path()).expect("set cwd");
        let resolved = resolve_file_path("a.rs").expect("resolve");
        std::env::set_current_dir(original_cwd).expect("restore cwd");

        assert!(resolved.ends_with("a.rs"));
    }
}
