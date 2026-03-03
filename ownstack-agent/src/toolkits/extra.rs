use crate::toolkits::{ToolDef, ToolResult, Toolkit, ToolkitError};
use async_trait::async_trait;
use serde_json::json;
use tracing::info;

/// Extra tools including browser simulation and delegate task.
pub struct ExtraToolkit;

/// Specialist role mapping for delegate_task.
fn specialist_system_prompt(role: &str) -> &'static str {
    match role {
        "pm" => "You are a Product Manager. Analyze requirements, write user stories, and prioritize features.",
        "engineer" => "You are a Senior Engineer. Write clean, tested, production-ready code.",
        "designer" => "You are a UX Designer. Audit UX flows, generate color palettes, and suggest UI improvements.",
        "reviewer" => "You are a Code Reviewer. Find bugs, security issues, and suggest improvements.",
        "qa" => "You are a QA Engineer. Write test plans, edge cases, and verify correctness.",
        "security" => "You are a Security Engineer. Audit code for vulnerabilities, injection flaws, and unsafe patterns.",
        "docs" => "You are a Technical Writer. Write clear, concise documentation and API references.",
        _ => "You are a helpful assistant.",
    }
}

#[async_trait]
impl Toolkit for ExtraToolkit {
    fn name(&self) -> &str {
        "extra"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef {
                name: "delegate_task".to_string(),
                description: "Delegate a task to a specialist agent (PM, Engineer, Designer, Reviewer, QA, Security, Docs). The specialist will analyze the task from their perspective and return structured recommendations.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "role": {
                            "type": "string",
                            "description": "Target specialist role",
                            "enum": ["pm", "engineer", "designer", "reviewer", "qa", "security", "docs"]
                        },
                        "instructions": {"type": "string", "description": "Detailed instructions for the specialist"},
                        "context": {"type": "string", "description": "Optional additional context (file contents, error logs, etc.)"},
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
            "delegate_task" => {
                let role = args["role"].as_str().unwrap_or("unknown");
                let instructions = args["instructions"].as_str().unwrap_or("");
                let context = args["context"].as_str().unwrap_or("");
                let system_prompt = specialist_system_prompt(role);

                info!("ExtraToolkit: delegating task to specialist '{role}'");

                // Build the specialist's analysis prompt
                let analysis = format!(
                    "## Specialist Analysis ({role})\n\n\
                     **System Role**: {system_prompt}\n\n\
                     **Task**: {instructions}\n\n\
                     {}\n\
                     **Recommendation**: This task has been routed to the {role} specialist.\n\
                     To get a full analysis, the orchestrator should invoke an LLM call with \
                     this specialist's system prompt and the provided instructions.\n\n\
                     **Next Steps**:\n\
                     1. The specialist will analyze the task from their perspective\n\
                     2. They will provide structured recommendations\n\
                     3. The main agent can incorporate or reject the suggestions",
                    if !context.is_empty() {
                        format!("**Context**: {context}\n\n")
                    } else {
                        String::new()
                    }
                );

                Ok(ToolResult::success(analysis))
            }
            _ => Err(ToolkitError::ToolNotFound(tool_name.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_delegate_task_pm() {
        let toolkit = ExtraToolkit;
        let result = toolkit
            .execute(
                "delegate_task",
                json!({
                    "role": "pm",
                    "instructions": "Prioritize the backlog for the auth module",
                }),
            )
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("pm"));
        assert!(result.stdout.contains("Product Manager"));
    }

    #[tokio::test]
    async fn test_delegate_task_with_context() {
        let toolkit = ExtraToolkit;
        let result = toolkit
            .execute(
                "delegate_task",
                json!({
                    "role": "security",
                    "instructions": "Audit the login flow",
                    "context": "The auth module is in src/auth.rs",
                }),
            )
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("Security Engineer"));
        assert!(result.stdout.contains("src/auth.rs"));
    }

    #[tokio::test]
    async fn test_delegate_task_unknown_tool() {
        let toolkit = ExtraToolkit;
        let result = toolkit.execute("nonexistent", json!({})).await;
        assert!(result.is_err());
    }
}
