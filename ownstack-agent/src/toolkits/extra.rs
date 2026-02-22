use crate::toolkits::{ToolDef, ToolResult, Toolkit, ToolkitError};
use async_trait::async_trait;
use serde_json::json;

pub struct ExtraToolkit;

#[async_trait]
impl Toolkit for ExtraToolkit {
    fn name(&self) -> &str {
        "extra"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef {
                name: "browse_url".to_string(),
                description:
                    "Naviguer sur une URL et interagir avec la page (Testing UI)"
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "url": {"type": "string", "description": "L'URL à consulter"},
                        "action": {"type": "string", "enum": ["navigate", "click", "type", "screenshot"], "default": "navigate"},
                        "selector": {"type": "string", "description": "Sélecteur CSS pour click/type"},
                        "text": {"type": "string", "description": "Texte à taper"},
                    },
                    "required": ["url"],
                }),
            },
            ToolDef {
                name: "delegate_task".to_string(),
                description: "Déléguer une tâche à un agent spécialiste".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "role": {
                            "type": "string",
                            "description": "Rôle cible",
                            "enum": ["pm", "engineer", "designer", "reviewer", "qa", "security", "docs"]
                        },
                        "instructions": {"type": "string", "description": "Instructions détaillées"},
                    },
                    "required": ["role", "instructions"],
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
            "browse_url" => {
                let url = args["url"].as_str().unwrap_or("unknown");
                Ok(ToolResult::success(format!(
                    "Simulated browsing of {}. Page title: 'Example'",
                    url
                )))
            }
            "delegate_task" => {
                let role = args["role"].as_str().unwrap_or("unknown");
                Ok(ToolResult::success(format!(
                    "Task delegated to specialist: {}. Waiting for response...",
                    role
                )))
            }
            _ => Err(ToolkitError::ToolNotFound(tool_name.to_string())),
        }
    }
}
