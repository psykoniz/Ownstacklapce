//! OpenRouter LLM Provider
//!
//! Provides access to multiple models via the OpenRouter API,
//! including Claude, GPT-4, and open-source models.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::provider::{
    FinishReason, LlmMessage, LlmProvider, LlmResponse, ProviderConfig, ProviderError,
    Role, TokenUsage, ToolCall, ToolDefinition,
};
use crate::resilience::ResilientClient;

const OPENROUTER_API_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

/// OpenRouter API provider
pub struct OpenRouterProvider {
    client: ResilientClient,
    config: ProviderConfig,
}

impl OpenRouterProvider {
    pub fn new(config: ProviderConfig) -> Self {
        let client = ResilientClient::new(config.retry.clone());
        Self {
            client,
            config,
        }
    }

    pub fn from_env() -> Result<Self, ProviderError> {
        let api_key = std::env::var("OPENROUTER_API_KEY")
            .map_err(|_| ProviderError::ConfigError("OPENROUTER_API_KEY not set".to_string()))?;

        let model = std::env::var("OPENROUTER_MODEL")
            .unwrap_or_else(|_| "anthropic/claude-3.5-sonnet".to_string());

        Ok(Self::new(ProviderConfig {
            api_key,
            model,
            base_url: Some(OPENROUTER_API_URL.to_string()),
            ..Default::default()
        }))
    }
}

#[derive(Serialize)]
struct OpenRouterRequest {
    model: String,
    messages: Vec<OpenRouterMessage>,
    max_tokens: u32,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenRouterTool>>,
}

#[derive(Serialize)]
struct OpenRouterMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Serialize)]
struct OpenRouterTool {
    #[serde(rename = "type")]
    tool_type: String,
    function: OpenRouterFunction,
}

#[derive(Serialize)]
struct OpenRouterFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Deserialize)]
struct OpenRouterResponse {
    choices: Vec<OpenRouterChoice>,
    usage: Option<OpenRouterUsage>,
}

#[derive(Deserialize)]
struct OpenRouterChoice {
    message: OpenRouterResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OpenRouterResponseMessage {
    content: Option<String>,
    tool_calls: Option<Vec<OpenRouterToolCall>>,
}

#[derive(Deserialize)]
struct OpenRouterToolCall {
    id: String,
    function: OpenRouterFunctionCall,
}

#[derive(Deserialize)]
struct OpenRouterFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct OpenRouterUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

fn role_to_string(role: &Role) -> String {
    match role {
        Role::System => "system".to_string(),
        Role::User => "user".to_string(),
        Role::Assistant => "assistant".to_string(),
        Role::Tool => "tool".to_string(),
    }
}

#[async_trait]
impl LlmProvider for OpenRouterProvider {
    async fn complete(
        &self,
        messages: Vec<LlmMessage>,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<LlmResponse, ProviderError> {
        let api_messages: Vec<OpenRouterMessage> = messages
            .into_iter()
            .map(|m| OpenRouterMessage {
                role: role_to_string(&m.role),
                content: m.content,
                tool_call_id: m.tool_call_id,
            })
            .collect();

        let api_tools = tools.map(|t| {
            t.into_iter()
                .map(|tool| OpenRouterTool {
                    tool_type: "function".to_string(),
                    function: OpenRouterFunction {
                        name: tool.name,
                        description: tool.description,
                        parameters: tool.parameters,
                    },
                })
                .collect()
        });

        let request = OpenRouterRequest {
            model: self.config.model.clone(),
            messages: api_messages,
            max_tokens: self.config.max_tokens,
            temperature: self.config.temperature,
            tools: api_tools,
        };

        debug!("Sending request to OpenRouter: model={}", self.config.model);

        let response = self
            .client
            .execute(
                self.client.inner()
                    .post(OPENROUTER_API_URL)
                    .header("Authorization", format!("Bearer {}", self.config.api_key))
                    .header("HTTP-Referer", "https://ownstack.dev")
                    .header("X-Title", "OwnStack IDE")
                    .json(&request)
            )
            .await?;

        let api_response: OpenRouterResponse = response
            .json()
            .await
            .map_err(|e| ProviderError::SerializationError(e.to_string()))?;

        let choice = api_response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| ProviderError::ApiError("No choices in response".to_string()))?;

        let tool_calls: Vec<ToolCall> = choice
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|tc| {
                let arguments: serde_json::Value =
                    serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null);
                ToolCall {
                    id: tc.id,
                    name: tc.function.name,
                    arguments,
                }
            })
            .collect();

        let finish_reason = match choice.finish_reason.as_deref() {
            Some("stop") => FinishReason::Stop,
            Some("tool_calls") => FinishReason::ToolCalls,
            Some("length") => FinishReason::Length,
            _ => FinishReason::Stop,
        };

        let usage = api_response.usage.map(|u| TokenUsage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        });

        Ok(LlmResponse {
            content: choice.message.content,
            tool_calls,
            finish_reason,
            usage,
        })
    }

    fn name(&self) -> &str {
        "openrouter"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    async fn stream(
        &self,
        messages: Vec<LlmMessage>,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<crate::provider::StreamResult, ProviderError> {
        use futures::StreamExt;

        let api_messages: Vec<OpenRouterMessage> = messages
            .into_iter()
            .map(|m| OpenRouterMessage {
                role: role_to_string(&m.role),
                content: m.content,
                tool_call_id: m.tool_call_id,
            })
            .collect();

        let api_tools = tools.map(|t| {
            t.into_iter()
                .map(|tool| OpenRouterTool {
                    tool_type: "function".to_string(),
                    function: OpenRouterFunction {
                        name: tool.name,
                        description: tool.description,
                        parameters: tool.parameters,
                    },
                })
                .collect()
        });

        // Build request body with stream: true
        let mut body = serde_json::to_value(&OpenRouterRequest {
            model: self.config.model.clone(),
            messages: api_messages,
            max_tokens: self.config.max_tokens,
            temperature: self.config.temperature,
            tools: api_tools,
        })
        .map_err(|e| ProviderError::SerializationError(e.to_string()))?;
        body.as_object_mut().unwrap().insert("stream".to_string(), serde_json::Value::Bool(true));

        debug!("Streaming request to OpenRouter: model={}", self.config.model);

        let response = self
            .client
            .execute(
                self.client.inner()
                    .post(OPENROUTER_API_URL)
                    .header("Authorization", format!("Bearer {}", self.config.api_key))
                    .header("HTTP-Referer", "https://ownstack.dev")
                    .header("X-Title", "OwnStack IDE")
                    .json(&body)
            )
            .await?;

        let byte_stream = response.bytes_stream();

        let stream = futures::stream::unfold(
            (byte_stream, String::new()),
            |(mut byte_stream, mut buffer)| async move {
                use futures::StreamExt;
                loop {
                    // Check if we have a complete line in the buffer
                    if let Some(newline_pos) = buffer.find('\n') {
                        let line = buffer[..newline_pos].trim().to_string();
                        buffer = buffer[newline_pos + 1..].to_string();

                        if line.is_empty() {
                            continue;
                        }

                        if let Some(data) = line.strip_prefix("data: ") {
                            if data.trim() == "[DONE]" {
                                return None;
                            }

                            match serde_json::from_str::<serde_json::Value>(data) {
                                Ok(json) => {
                                    if let Some(chunk) = parse_sse_chunk(&json) {
                                        return Some((Ok(chunk), (byte_stream, buffer)));
                                    }
                                    continue;
                                }
                                Err(e) => {
                                    return Some((
                                        Err(ProviderError::StreamError(e.to_string())),
                                        (byte_stream, buffer),
                                    ));
                                }
                            }
                        }
                        continue;
                    }

                    // Need more data
                    match byte_stream.next().await {
                        Some(Ok(bytes)) => {
                            buffer.push_str(&String::from_utf8_lossy(&bytes));
                        }
                        Some(Err(e)) => {
                            return Some((
                                Err(ProviderError::StreamError(e.to_string())),
                                (byte_stream, buffer),
                            ));
                        }
                        None => return None,
                    }
                }
            },
        );

        Ok(Box::pin(stream))
    }
}

/// Parse an OpenRouter/OpenAI SSE chunk JSON into a StreamChunk
fn parse_sse_chunk(json: &serde_json::Value) -> Option<crate::provider::StreamChunk> {
    use crate::provider::{StreamChunk, ToolCallDelta, FinishReason};

    let choices = json.get("choices")?.as_array()?;
    let choice = choices.first()?;
    let delta = choice.get("delta")?;

    let delta_content = delta.get("content").and_then(|c| c.as_str()).map(|s| s.to_string());

    let delta_tool_calls: Vec<ToolCallDelta> = delta
        .get("tool_calls")
        .and_then(|tc| tc.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|tc| {
                    let index = tc.get("index")?.as_u64()? as usize;
                    let id = tc.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let func = tc.get("function")?;
                    let name = func.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let arguments_delta = func
                        .get("arguments")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    Some(ToolCallDelta { index, id, name, arguments_delta })
                })
                .collect()
        })
        .unwrap_or_default();

    let finish_reason = choice
        .get("finish_reason")
        .and_then(|fr| fr.as_str())
        .map(|s| match s {
            "stop" => FinishReason::Stop,
            "tool_calls" => FinishReason::ToolCalls,
            "length" => FinishReason::Length,
            _ => FinishReason::Stop,
        });

    let usage = json.get("usage").and_then(|u| {
        Some(crate::provider::TokenUsage {
            prompt_tokens: u.get("prompt_tokens")?.as_u64()? as u32,
            completion_tokens: u.get("completion_tokens")?.as_u64()? as u32,
            total_tokens: u.get("total_tokens")?.as_u64()? as u32,
        })
    });

    Some(StreamChunk {
        delta_content,
        delta_tool_calls,
        finish_reason,
        usage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_conversion() {
        assert_eq!(role_to_string(&Role::System), "system");
        assert_eq!(role_to_string(&Role::User), "user");
        assert_eq!(role_to_string(&Role::Assistant), "assistant");
        assert_eq!(role_to_string(&Role::Tool), "tool");
    }

    #[test]
    fn test_provider_name() {
        let config = ProviderConfig {
            api_key: "test".to_string(),
            model: "test".to_string(),
            ..Default::default()
        };
        let provider = OpenRouterProvider::new(config);
        assert_eq!(provider.name(), "openrouter");
    }

    #[test]
    fn test_request_serialization() {
        let messages = vec![LlmMessage {
            role: Role::User,
            content: "hello".to_string(),
            tool_call_id: None,
            tool_calls: None,
        }];
        
        let api_messages: Vec<OpenRouterMessage> = messages
            .into_iter()
            .map(|m| OpenRouterMessage {
                role: role_to_string(&m.role),
                content: m.content,
                tool_call_id: m.tool_call_id,
            })
            .collect();

        let request = OpenRouterRequest {
            model: "gpt-4".to_string(),
            messages: api_messages,
            max_tokens: 100,
            temperature: 0.7,
            tools: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("\"content\":\"hello\""));
    }
}
