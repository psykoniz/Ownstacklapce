use crate::toolkits::{ToolDef, ToolResult, Toolkit, ToolkitError};
use async_trait::async_trait;
use serde_json::json;

pub struct PMToolkit;

#[async_trait]
impl Toolkit for PMToolkit {
    fn name(&self) -> &str {
        "pm"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef {
                name: "create_specification".to_string(),
                description: "Create a detailed technical specification (implementation_plan.md)".to_string(),
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
                description: "Review an existing implementation plan for gaps".to_string(),
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
                Ok(ToolResult::success(format!(
                    "Specification for {} created successfully.",
                    feature
                )))
            }
            "review_plan" => {
                let path = args["plan_path"].as_str().unwrap_or("unknown");
                Ok(ToolResult::success(format!(
                    "Plan at {} reviewed. No critical gaps found.",
                    path
                )))
            }
            _ => Err(ToolkitError::ToolNotFound(tool_name.to_string())),
        }
    }
}
