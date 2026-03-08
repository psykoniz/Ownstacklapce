use crate::provider::{LlmMessage, LlmProvider, ProviderOptions};
use crate::toolkits::{ToolDef, ToolResult, Toolkit, ToolkitError};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use tracing::{info, warn};

/// Extra tools including specialist delegation with real LLM routing.
pub struct ExtraToolkit {
    provider: Option<Arc<dyn LlmProvider + Send + Sync>>,
}

impl ExtraToolkit {
    pub fn new(provider: Option<Arc<dyn LlmProvider + Send + Sync>>) -> Self {
        Self { provider }
    }
}

impl Default for ExtraToolkit {
    fn default() -> Self {
        Self { provider: None }
    }
}

/// Specialist role mapping for delegate_task.
fn specialist_system_prompt(role: &str) -> &'static str {
    match role {
        "pm" => "You are a Product Manager. Analyze requirements, write user stories, and prioritize features. Respond with structured markdown.",
        "engineer" => "You are a Senior Engineer. Write clean, tested, production-ready code. Provide code examples and implementation plans.",
        "designer" => "You are a UX Designer. Audit UX flows, generate color palettes, and suggest UI improvements. Use wireframe descriptions.",
        "reviewer" => "You are a Code Reviewer. Find bugs, security issues, and suggest improvements. Rate severity of each finding.",
        "qa" => "You are a QA Engineer. Write test plans, edge cases, and verify correctness. Structure your response as a test matrix.",
        "security" => "You are a Security Engineer. Audit code for vulnerabilities, injection flaws, and unsafe patterns. Use OWASP categories.",
        "docs" => "You are a Technical Writer. Write clear, concise documentation and API references. Follow the Divio documentation system.",
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
                description: "Delegate a task to a specialist agent (PM, Engineer, Designer, Reviewer, QA, Security, Docs). Uses LLM with specialist system prompt for real analysis.".to_string(),
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

                // Build the user message
                let user_msg = if context.is_empty() {
                    format!("**Task**: {instructions}")
                } else {
                    format!(
                        "**Task**: {instructions}\n\n**Context**:\n```\n{context}\n```"
                    )
                };

                // Try LLM routing if provider is available
                if let Some(provider) = &self.provider {
                    let messages = vec![
                        LlmMessage::system(system_prompt),
                        LlmMessage::user(&user_msg),
                    ];

                    match provider
                        .complete(messages, None, ProviderOptions::default())
                        .await
                    {
                        Ok(response) => {
                            let content = response
                                .content
                                .unwrap_or_else(|| "(no response)".to_string());
                            let analysis = format!(
                                "## Specialist Analysis ({role})\n\n{content}"
                            );
                            let mut result = ToolResult::success(analysis);
                            result.metadata.insert(
                                "specialist_role".to_string(),
                                role.to_string(),
                            );
                            result.metadata.insert(
                                "llm_routed".to_string(),
                                "true".to_string(),
                            );
                            return Ok(result);
                        }
                        Err(err) => {
                            warn!(
                                "ExtraToolkit: LLM routing failed for '{role}': {err}, falling back to template"
                            );
                        }
                    }
                }

                // Fallback: structured template without LLM
                let analysis = format!(
                    "## Specialist Analysis ({role})\n\n\
                     **System Role**: {system_prompt}\n\n\
                     **Task**: {instructions}\n\n\
                     {}\
                     **Recommendation**: This task has been routed to the {role} specialist.\n\
                     LLM provider is not configured — returning template response.\n\n\
                     **Next Steps**:\n\
                     1. Configure an LLM API key for real specialist analysis\n\
                     2. The specialist will analyze the task from their perspective\n\
                     3. They will provide structured recommendations",
                    if !context.is_empty() {
                        format!("**Context**: {context}\n\n")
                    } else {
                        String::new()
                    }
                );

                let mut result = ToolResult::success(analysis);
                result
                    .metadata
                    .insert("specialist_role".to_string(), role.to_string());
                result
                    .metadata
                    .insert("llm_routed".to_string(), "false".to_string());
                Ok(result)
            }
            _ => Err(ToolkitError::ToolNotFound(tool_name.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_delegate_task_pm_fallback() {
        let toolkit = ExtraToolkit::default();
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
        assert_eq!(
            result.metadata.get("llm_routed").map(|s| s.as_str()),
            Some("false")
        );
    }

    #[tokio::test]
    async fn test_delegate_task_with_context() {
        let toolkit = ExtraToolkit::default();
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
        let toolkit = ExtraToolkit::default();
        let result = toolkit.execute("nonexistent", json!({})).await;
        assert!(result.is_err());
    }
}
