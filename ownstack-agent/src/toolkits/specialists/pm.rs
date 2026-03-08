//! Product Manager Specialist — generates specs and reviews plans using
//! workspace context and structured templates.

use crate::toolkits::{ToolDef, ToolResult, Toolkit, ToolkitError};
use async_trait::async_trait;
use ownstack_engine::PathValidator;
use serde_json::json;
use std::path::Path;
use std::path::PathBuf;
use tracing::info;

pub struct PMToolkit {
    workspace: PathBuf,
    path_validator: PathValidator,
}

impl PMToolkit {
    pub fn new(workspace: PathBuf) -> Self {
        let path_validator = PathValidator::new(workspace.clone());
        Self {
            workspace,
            path_validator,
        }
    }
}

impl Default for PMToolkit {
    fn default() -> Self {
        let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            path_validator: PathValidator::new(workspace.clone()),
            workspace,
        }
    }
}

fn sanitize_feature_slug(feature: &str) -> String {
    let mut slug = feature
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else if matches!(c, '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();

    while slug.contains("__") {
        slug = slug.replace("__", "_");
    }

    let slug = slug.trim_matches('_').to_string();
    let slug = if slug.is_empty() {
        "specification".to_string()
    } else {
        slug
    };
    slug.chars().take(96).collect()
}

#[async_trait]
impl Toolkit for PMToolkit {
    fn name(&self) -> &str {
        "pm"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef {
                name: "create_specification".to_string(),
                description: "Create a detailed technical specification document"
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "feature_name": {"type": "string", "description": "Name of the feature"},
                        "requirements": {"type": "string", "description": "Raw user requirements"},
                    },
                    "required": ["feature_name", "requirements"],
                }),
            },
            ToolDef {
                name: "review_plan".to_string(),
                description: "Review an existing implementation plan for gaps"
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "plan_path": {"type": "string", "description": "Path to the plan file"},
                    },
                    "required": ["plan_path"],
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
            "create_specification" => {
                let feature = args["feature_name"].as_str().unwrap_or("unknown");
                let requirements = args["requirements"].as_str().unwrap_or("");

                info!("PMToolkit: creating specification for '{feature}'");

                let project_files = scan_project_structure(&self.workspace);

                let spec = format!(
                    "# Technical Specification: {feature}\n\n\
                     ## 1. Overview\n\
                     **Feature**: {feature}\n\
                     **Status**: Draft\n\n\
                     ## 2. Requirements\n\
                     {requirements}\n\n\
                     ## 3. Project Context\n\
                     {project_files}\n\n\
                     ## 4. Acceptance Criteria\n\
                     - [ ] Feature implements all stated requirements\n\
                     - [ ] Unit tests cover core logic\n\
                     - [ ] Integration tests validate end-to-end flow\n\
                     - [ ] No security regressions\n\
                     - [ ] Documentation updated\n\n\
                     ## 5. Implementation Notes\n\
                     - Identify affected modules from the project structure above\n\
                     - Ensure backward compatibility with existing APIs\n\
                     - Add feature flag if the change is large\n"
                );

                // Write spec to workspace (path-validated, traversal-safe).
                let spec_file = format!("{}.md", sanitize_feature_slug(feature));
                let spec_rel = PathBuf::from(".ownstack").join("specs").join(spec_file);
                let spec_path = self
                    .path_validator
                    .validate(Path::new(&spec_rel))
                    .map_err(|e| ToolkitError::SecurityViolation(e.to_string()))?;

                if let Some(parent) = spec_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        ToolkitError::ExecutionFailed(format!(
                            "Failed to create spec directory: {e}"
                        ))
                    })?;
                }
                std::fs::write(&spec_path, &spec).map_err(|e| {
                    ToolkitError::ExecutionFailed(format!(
                        "Failed to write spec file: {e}"
                    ))
                })?;

                let mut result = ToolResult::success(spec);
                result.metadata.insert(
                    "spec_path".to_string(),
                    spec_path.to_string_lossy().to_string(),
                );
                Ok(result)
            }
            "review_plan" => {
                let path = args["plan_path"].as_str().unwrap_or("");
                if path.is_empty() {
                    return Err(ToolkitError::InvalidArguments(
                        "plan_path is required".to_string(),
                    ));
                }

                info!("PMToolkit: reviewing plan at '{path}'");

                let full_path = self
                    .path_validator
                    .validate(Path::new(path))
                    .map_err(|e| ToolkitError::SecurityViolation(e.to_string()))?;
                let content = std::fs::read_to_string(&full_path).map_err(|e| {
                    ToolkitError::ExecutionFailed(format!("Cannot read plan: {e}"))
                })?;

                let mut issues = Vec::new();
                let lines: Vec<&str> = content.lines().collect();

                if !content.contains("## ") && !content.contains("# ") {
                    issues.push("Missing section headers — plan lacks structure");
                }
                if !content.to_lowercase().contains("test") {
                    issues.push("No testing strategy mentioned");
                }
                if !content.to_lowercase().contains("risk")
                    && !content.to_lowercase().contains("rollback")
                {
                    issues.push("No risk assessment or rollback plan");
                }
                if !content.to_lowercase().contains("security") {
                    issues.push("Security considerations not addressed");
                }
                if lines.len() < 10 {
                    issues.push("Plan is very short — may lack detail");
                }

                let review = if issues.is_empty() {
                    format!(
                        "## Plan Review: {path}\n\n\
                         **Verdict**: LGTM — no critical gaps found.\n\
                         **Lines**: {}\n",
                        lines.len()
                    )
                } else {
                    format!(
                        "## Plan Review: {path}\n\n\
                         **Verdict**: Needs revision — {} issue(s) found.\n\n\
                         ### Issues\n{}\n",
                        issues.len(),
                        issues
                            .iter()
                            .map(|i| format!("- {i}"))
                            .collect::<Vec<_>>()
                            .join("\n")
                    )
                };

                Ok(ToolResult::success(review))
            }
            _ => Err(ToolkitError::ToolNotFound(tool_name.to_string())),
        }
    }
}

/// Quick scan of workspace top-level structure for context.
fn scan_project_structure(workspace: &PathBuf) -> String {
    let mut entries = Vec::new();
    if let Ok(dir) = std::fs::read_dir(workspace) {
        for entry in dir.flatten().take(30) {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let kind = if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                "dir"
            } else {
                "file"
            };
            entries.push(format!("- {name} ({kind})"));
        }
    }
    if entries.is_empty() {
        "No project files found.".to_string()
    } else {
        entries.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolkits::Toolkit;

    #[tokio::test]
    async fn review_plan_blocks_parent_traversal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let toolkit = PMToolkit::new(dir.path().to_path_buf());

        let result = toolkit
            .execute("review_plan", json!({"plan_path":"../outside.md"}))
            .await;
        assert!(matches!(result, Err(ToolkitError::SecurityViolation(_))));
    }

    #[tokio::test]
    async fn create_specification_writes_inside_workspace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let toolkit = PMToolkit::new(dir.path().to_path_buf());

        let result = toolkit
            .execute(
                "create_specification",
                json!({
                    "feature_name":"../../escape",
                    "requirements":"must stay inside workspace"
                }),
            )
            .await
            .expect("create specification");

        let spec_path = result
            .metadata
            .get("spec_path")
            .expect("spec_path metadata");
        let spec_path = PathBuf::from(spec_path);
        let normalized = spec_path.to_string_lossy().replace('\\', "/");
        assert!(normalized.contains("/.ownstack/specs/"));
        assert!(spec_path.exists());
    }
}
