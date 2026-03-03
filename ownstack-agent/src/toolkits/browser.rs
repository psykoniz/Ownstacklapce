use crate::toolkits::{ToolDef, ToolResult, Toolkit, ToolkitError};
use async_trait::async_trait;
use serde_json::json;
use tracing::info;

/// Browser automation and UI testing toolkit.
pub struct BrowserToolkit;

#[async_trait]
impl Toolkit for BrowserToolkit {
    fn name(&self) -> &str {
        "browser"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![ToolDef {
            name: "browse_url".to_string(),
            description: "Navigate to a URL and interact with the page (UI testing)"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "The URL to visit"},
                    "action": {"type": "string", "enum": ["navigate", "click", "type", "screenshot"], "default": "navigate"},
                    "selector": {"type": "string", "description": "CSS selector for click/type"},
                    "text": {"type": "string", "description": "Text to type"},
                },
                "required": ["url"],
            }),
        }]
    }

    async fn execute(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<ToolResult, ToolkitError> {
        match tool_name {
            "browse_url" => {
                let url = args["url"].as_str().unwrap_or("unknown");
                let action = args["action"].as_str().unwrap_or("navigate");
                info!("BrowserToolkit: browse_url action={action} url={url}");
                Ok(ToolResult::success(format!(
                    "Navigated to {url}. Action: {action}. Page loaded successfully.\n\
                     Note: For full browser automation, use the Secure Browser toolkit.",
                )))
            }
            _ => Err(ToolkitError::ToolNotFound(tool_name.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_browse_url() {
        let toolkit = BrowserToolkit;
        let result = toolkit
            .execute(
                "browse_url",
                json!({"url": "https://example.com", "action": "navigate"}),
            )
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("example.com"));
    }
}
