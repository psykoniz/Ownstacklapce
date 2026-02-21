//! Anthropic Claude LLM Provider
//!
//! Direct integration with the Anthropic API for Claude models.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::provider::{
    FinishReason, LlmMessage, LlmProvider, LlmResponse, ProviderConfig,
    ProviderError, Role, TokenUsage, ToolCall, ToolDefinition,
};
use crate::resilience::ResilientClient;
use crate::secret_store;

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Anthropic Claude API provider
pub struct AnthropicProvider {
    client: ResilientClient,
    config: ProviderConfig,
}

impl AnthropicProvider {
    pub fn new(config: ProviderConfig) -> Self {
        let client = ResilientClient::new(config.retry.clone());
        Self { client, config }
    }

    pub fn from_env() -> Result<Self, ProviderError> {
        let api_key =
            secret_store::get_secret("ANTHROPIC_API_KEY").ok_or_else(|| {
                ProviderError::ConfigError(
                    "ANTHROPIC_API_KEY not set (env/keyring)".to_string(),
                )
            })?;

        let model = std::env::var("ANTHROPIC_MODEL")
            .unwrap_or_else(|_| "claude-3-5-sonnet-20241022".to_string());

        Ok(Self::new(ProviderConfig {
            api_key,
            model,
            base_url: Some(ANTHROPIC_API_URL.to_string()),
            ..Default::default()
        }))
    }
}

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: String,
    content: AnthropicContent,
}

#[derive(Serialize)]
#[serde(untagged)]
enum AnthropicContent {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

#[derive(Serialize)]
#[serde(tag = "type")]
#[allow(dead_code)]
enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
    #[serde(rename = "image")]
    Image {
        source: AnthropicImageSource,
    },
}

#[derive(Serialize)]
pub struct AnthropicImageSource {
    #[serde(rename = "type")]
    pub type_: String,
    pub media_type: String,
    pub data: String,
}

#[derive(Serialize)]
struct AnthropicTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicResponseContent>,
    stop_reason: Option<String>,
    usage: AnthropicUsage,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum AnthropicResponseContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn complete(
        &self,
        messages: Vec<LlmMessage>,
        tools: Option<Vec<ToolDefinition>>,
        model_override: Option<String>,
    ) -> Result<LlmResponse, ProviderError> {
        // Extract system message
        let system_msg = messages
            .iter()
            .find(|m| m.role == Role::System)
            .map(|m| m.get_text());

        // Convert messages (excluding system)
        let api_messages: Vec<AnthropicMessage> = messages
            .into_iter()
            .filter(|m| m.role != Role::System)
            .map(|m| {
                let role = match m.role {
                    Role::User | Role::Tool => "user",
                    Role::Assistant => "assistant",
                    Role::System => unreachable!(),
                };

                let content = match m.role {
                    Role::Tool => {
                        AnthropicContent::Blocks(vec![
                            AnthropicContentBlock::ToolResult {
                                tool_use_id: m.tool_call_id.clone().unwrap_or_default(),
                                content: m.get_text(),
                            },
                        ])
                    }
                    _ => match m.content {
                        crate::provider::MessageContent::Text(s) => AnthropicContent::Text(s),
                        crate::provider::MessageContent::Parts(parts) => {
                            AnthropicContent::Blocks(parts.into_iter().map(|p| match p {
                                crate::provider::ContentPart::Text { text } => {
                                    AnthropicContentBlock::Text { text }
                                }
                                crate::provider::ContentPart::Image { source } => {
                                    AnthropicContentBlock::Image {
                                        source: AnthropicImageSource {
                                            type_: source.type_,
                                            media_type: source.media_type,
                                            data: source.data,
                                        },
                                    }
                                }
                            }).collect())
                        }
                    },
                };

                AnthropicMessage {
                    role: role.to_string(),
                    content,
                }
            })
            .collect();

        let api_tools = tools.map(|t| {
            t.into_iter()
                .map(|tool| AnthropicTool {
                    name: tool.name,
                    description: tool.description,
                    input_schema: tool.parameters,
                })
                .collect()
        });

        let request = AnthropicRequest {
            model: model_override.unwrap_or_else(|| self.config.model.clone()),
            max_tokens: self.config.max_tokens,
            messages: api_messages,
            system: system_msg,
            tools: api_tools,
        };

        debug!("Sending request to Anthropic: model={}", self.config.model);

        let response = self
            .client
            .execute(
                self.client
                    .inner()
                    .post(ANTHROPIC_API_URL)
                    .header("x-api-key", &self.config.api_key)
                    .header("anthropic-version", ANTHROPIC_VERSION)
                    .header("content-type", "application/json")
                    .json(&request),
            )
            .await?;

        let api_response: AnthropicResponse = response
            .json()
            .await
            .map_err(|e| ProviderError::SerializationError(e.to_string()))?;

        let mut content_text = String::new();
        let mut tool_calls = Vec::new();

        for block in api_response.content {
            match block {
                AnthropicResponseContent::Text { text } => {
                    content_text.push_str(&text);
                }
                AnthropicResponseContent::ToolUse { id, name, input } => {
                    tool_calls.push(ToolCall {
                        id,
                        name,
                        arguments: input,
                    });
                }
            }
        }

        let finish_reason = match api_response.stop_reason.as_deref() {
            Some("end_turn") => FinishReason::Stop,
            Some("tool_use") => FinishReason::ToolCalls,
            Some("max_tokens") => FinishReason::Length,
            _ => FinishReason::Stop,
        };

        let usage = TokenUsage {
            prompt_tokens: api_response.usage.input_tokens,
            completion_tokens: api_response.usage.output_tokens,
            total_tokens: api_response.usage.input_tokens
                + api_response.usage.output_tokens,
        };

        Ok(LlmResponse {
            content: if content_text.is_empty() {
                None
            } else {
                Some(content_text)
            },
            tool_calls,
            finish_reason,
            usage: Some(usage),
        })
    }

    fn name(&self) -> &str {
        "anthropic"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    async fn stream(
        &self,
        messages: Vec<LlmMessage>,
        tools: Option<Vec<ToolDefinition>>,
        model_override: Option<String>,
    ) -> Result<crate::provider::StreamResult, ProviderError> {
        use crate::provider::{FinishReason, StreamChunk, ToolCallDelta};

        // Extract system message
        let system_msg = messages
            .iter()
            .find(|m| m.role == Role::System)
            .map(|m| m.get_text());

        let api_messages: Vec<AnthropicMessage> = messages
            .into_iter()
            .filter(|m| m.role != Role::System)
            .map(|m| {
                let role = match m.role {
                    Role::User | Role::Tool => "user",
                    Role::Assistant => "assistant",
                    Role::System => unreachable!(),
                };
                let content = if m.role == Role::Tool {
                    AnthropicContent::Blocks(vec![
                        AnthropicContentBlock::ToolResult {
                            tool_use_id: m.tool_call_id.clone().unwrap_or_default(),
                            content: m.get_text(),
                        },
                    ])
                } else {
                    AnthropicContent::Text(m.get_text())
                };
                AnthropicMessage {
                    role: role.to_string(),
                    content,
                }
            })
            .collect();

        let api_tools = tools.map(|t| {
            t.into_iter()
                .map(|tool| AnthropicTool {
                    name: tool.name,
                    description: tool.description,
                    input_schema: tool.parameters,
                })
                .collect()
        });

        let mut body = serde_json::to_value(&AnthropicRequest {
            model: model_override.unwrap_or_else(|| self.config.model.clone()),
            max_tokens: self.config.max_tokens,
            messages: api_messages,
            system: system_msg,
            tools: api_tools,
        })
        .map_err(|e| ProviderError::SerializationError(e.to_string()))?;
        body.as_object_mut()
            .unwrap()
            .insert("stream".to_string(), serde_json::Value::Bool(true));

        debug!(
            "Streaming request to Anthropic: model={}",
            self.config.model
        );

        let response = self
            .client
            .execute(
                self.client
                    .inner()
                    .post(ANTHROPIC_API_URL)
                    .header("x-api-key", &self.config.api_key)
                    .header("anthropic-version", ANTHROPIC_VERSION)
                    .header("content-type", "application/json")
                    .json(&body),
            )
            .await?;

        let byte_stream = response.bytes_stream();

        let stream = futures::stream::unfold(
            (byte_stream, String::new(), None::<String>),
            |(mut byte_stream, mut buffer, mut current_event)| async move {
                use futures::StreamExt;
                loop {
                    if let Some(newline_pos) = buffer.find('\n') {
                        let line = buffer[..newline_pos].trim().to_string();
                        buffer = buffer[newline_pos + 1..].to_string();

                        if line.is_empty() {
                            continue;
                        }

                        if let Some(event_type) = line.strip_prefix("event: ") {
                            current_event = Some(event_type.to_string());
                            continue;
                        }

                        if let Some(data) = line.strip_prefix("data: ") {
                            let event = current_event.take().unwrap_or_default();

                            match event.as_str() {
                                "content_block_delta" => {
                                    if let Ok(json) =
                                        serde_json::from_str::<serde_json::Value>(
                                            data,
                                        )
                                    {
                                        let delta = json.get("delta");
                                        let delta_type = delta
                                            .and_then(|d| d.get("type"))
                                            .and_then(|t| t.as_str());

                                        match delta_type {
                                            Some("text_delta") => {
                                                let text = delta
                                                    .and_then(|d| d.get("text"))
                                                    .and_then(|t| t.as_str())
                                                    .map(|s| s.to_string());
                                                let chunk = StreamChunk {
                                                    delta_content: text,
                                                    delta_tool_calls: vec![],
                                                    finish_reason: None,
                                                    usage: None,
                                                };
                                                return Some((
                                                    Ok(chunk),
                                                    (
                                                        byte_stream,
                                                        buffer,
                                                        current_event,
                                                    ),
                                                ));
                                            }
                                            Some("input_json_delta") => {
                                                let partial = delta
                                                    .and_then(|d| {
                                                        d.get("partial_json")
                                                    })
                                                    .and_then(|t| t.as_str())
                                                    .map(|s| s.to_string());
                                                let index = json
                                                    .get("index")
                                                    .and_then(|i| i.as_u64())
                                                    .unwrap_or(0)
                                                    as usize;
                                                let chunk = StreamChunk {
                                                    delta_content: None,
                                                    delta_tool_calls: vec![
                                                        ToolCallDelta {
                                                            index,
                                                            id: None,
                                                            name: None,
                                                            arguments_delta: partial,
                                                        },
                                                    ],
                                                    finish_reason: None,
                                                    usage: None,
                                                };
                                                return Some((
                                                    Ok(chunk),
                                                    (
                                                        byte_stream,
                                                        buffer,
                                                        current_event,
                                                    ),
                                                ));
                                            }
                                            _ => continue,
                                        }
                                    }
                                }
                                "message_delta" => {
                                    if let Ok(json) =
                                        serde_json::from_str::<serde_json::Value>(
                                            data,
                                        )
                                    {
                                        let stop_reason = json
                                            .get("delta")
                                            .and_then(|d| d.get("stop_reason"))
                                            .and_then(|s| s.as_str())
                                            .map(|s| match s {
                                                "end_turn" => FinishReason::Stop,
                                                "tool_use" => {
                                                    FinishReason::ToolCalls
                                                }
                                                "max_tokens" => FinishReason::Length,
                                                _ => FinishReason::Stop,
                                            });
                                        let chunk = StreamChunk {
                                            delta_content: None,
                                            delta_tool_calls: vec![],
                                            finish_reason: stop_reason,
                                            usage: None,
                                        };
                                        return Some((
                                            Ok(chunk),
                                            (byte_stream, buffer, current_event),
                                        ));
                                    }
                                }
                                "message_stop" => return None,
                                _ => continue,
                            }
                        }
                        continue;
                    }

                    match byte_stream.next().await {
                        Some(Ok(bytes)) => {
                            buffer.push_str(&String::from_utf8_lossy(&bytes))
                        }
                        Some(Err(e)) => {
                            return Some((
                                Err(ProviderError::StreamError(e.to_string())),
                                (byte_stream, buffer, current_event),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anthropic_message_conversion() {
        let msg = LlmMessage {
            role: Role::User,
            content: "hello".into(),
            tool_call_id: None,
            tool_calls: None,
        };

        let api_msg = AnthropicMessage {
            role: "user".to_string(),
            content: AnthropicContent::Text(msg.get_text()),
        };

        let json = serde_json::to_string(&api_msg).unwrap();
        assert_eq!(json, r#"{"role":"user","content":"hello"}"#);
    }

    #[test]
    fn test_anthropic_tool_result_conversion() {
        let msg = LlmMessage {
            role: Role::Tool,
            content: "result".into(),
            tool_call_id: Some("call_123".to_string()),
            tool_calls: None,
        };

        let api_msg = AnthropicMessage {
            role: "user".to_string(),
            content: AnthropicContent::Blocks(vec![
                AnthropicContentBlock::ToolResult {
                    tool_use_id: msg.tool_call_id.clone().unwrap(),
                    content: msg.get_text(),
                },
            ]),
        };

        let json = serde_json::to_string(&api_msg).unwrap();
        assert!(json.contains("\"type\":\"tool_result\""));
        assert!(json.contains("\"tool_use_id\":\"call_123\""));
    }

    #[test]
    fn test_provider_name() {
        let config = ProviderConfig {
            api_key: "test".to_string(),
            model: "test".to_string(),
            ..Default::default()
        };
        let provider = AnthropicProvider::new(config);
        assert_eq!(provider.name(), "anthropic");
    }
}
