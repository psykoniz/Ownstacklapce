//! Local LLM Provider (Ollama)
//!
//! Integration with locally running models via Ollama API.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::provider::{
    FinishReason, LlmMessage, LlmProvider, LlmResponse, ProviderConfig, ProviderError,
    Role, TokenUsage, ToolCall, ToolDefinition,
};
use crate::resilience::ResilientClient;

const DEFAULT_OLLAMA_URL: &str = "http://localhost:11434";

/// Ollama local LLM provider
pub struct LocalProvider {
    client: ResilientClient,
    config: ProviderConfig,
    base_url: String,
}

impl LocalProvider {
    pub fn new(config: ProviderConfig) -> Self {
        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| DEFAULT_OLLAMA_URL.to_string());
        let client = ResilientClient::new(config.retry.clone());

        Self {
            client,
            config,
            base_url,
        }
    }

    pub fn from_env() -> Result<Self, ProviderError> {
        let base_url = std::env::var("OLLAMA_HOST")
            .unwrap_or_else(|_| DEFAULT_OLLAMA_URL.to_string());

        let model = std::env::var("OLLAMA_MODEL")
            .unwrap_or_else(|_| "llama3.2".to_string());

        Ok(Self::new(ProviderConfig {
            api_key: String::new(), // Ollama doesn't need API key
            model,
            base_url: Some(base_url),
            ..Default::default()
        }))
    }
}

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OllamaTool>>,
    options: OllamaOptions,
}

#[derive(Serialize)]
struct OllamaMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct OllamaTool {
    #[serde(rename = "type")]
    tool_type: String,
    function: OllamaFunction,
}

#[derive(Serialize)]
struct OllamaFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Serialize)]
struct OllamaOptions {
    temperature: f32,
    num_predict: u32,
}

#[derive(Deserialize)]
struct OllamaResponse {
    message: OllamaResponseMessage,
    _done: bool,
    #[serde(default)]
    prompt_eval_count: u32,
    #[serde(default)]
    eval_count: u32,
}

#[derive(Deserialize)]
struct OllamaResponseMessage {
    content: String,
    #[serde(default)]
    tool_calls: Vec<OllamaToolCall>,
}

#[derive(Deserialize)]
struct OllamaToolCall {
    function: OllamaFunctionCall,
}

#[derive(Deserialize)]
struct OllamaFunctionCall {
    name: String,
    arguments: serde_json::Value,
}

fn role_to_ollama(role: &Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

#[async_trait]
impl LlmProvider for LocalProvider {
    async fn complete(
        &self,
        messages: Vec<LlmMessage>,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<LlmResponse, ProviderError> {
        let api_messages: Vec<OllamaMessage> = messages
            .into_iter()
            .map(|m| OllamaMessage {
                role: role_to_ollama(&m.role).to_string(),
                content: m.content,
            })
            .collect();

        let api_tools = tools.map(|t| {
            t.into_iter()
                .map(|tool| OllamaTool {
                    tool_type: "function".to_string(),
                    function: OllamaFunction {
                        name: tool.name,
                        description: tool.description,
                        parameters: tool.parameters,
                    },
                })
                .collect()
        });

        let request = OllamaRequest {
            model: self.config.model.clone(),
            messages: api_messages,
            stream: false,
            tools: api_tools,
            options: OllamaOptions {
                temperature: self.config.temperature,
                num_predict: self.config.max_tokens,
            },
        };

        let url = format!("{}/api/chat", self.base_url);
        debug!("Sending request to Ollama: model={}", self.config.model);

        let response = self
            .client
            .execute(
                self.client.inner()
                    .post(&url)
                    .json(&request)
            )
            .await?;

        let api_response: OllamaResponse = response
            .json()
            .await
            .map_err(|e| ProviderError::SerializationError(e.to_string()))?;

        let tool_calls: Vec<ToolCall> = api_response
            .message
            .tool_calls
            .into_iter()
            .enumerate()
            .map(|(i, tc)| ToolCall {
                id: format!("call_{}", i),
                name: tc.function.name,
                arguments: tc.function.arguments,
            })
            .collect();

        let finish_reason = if !tool_calls.is_empty() {
            FinishReason::ToolCalls
        } else {
            FinishReason::Stop
        };

        let usage = TokenUsage {
            prompt_tokens: api_response.prompt_eval_count,
            completion_tokens: api_response.eval_count,
            total_tokens: api_response.prompt_eval_count + api_response.eval_count,
        };

        Ok(LlmResponse {
            content: if api_response.message.content.is_empty() {
                None
            } else {
                Some(api_response.message.content)
            },
            tool_calls,
            finish_reason,
            usage: Some(usage),
        })
    }

    fn name(&self) -> &str {
        "ollama"
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
        use crate::provider::{StreamChunk, FinishReason};

        let api_messages: Vec<OllamaMessage> = messages
            .into_iter()
            .map(|m| OllamaMessage {
                role: role_to_ollama(&m.role).to_string(),
                content: m.content,
            })
            .collect();

        let api_tools = tools.map(|t| {
            t.into_iter()
                .map(|tool| OllamaTool {
                    tool_type: "function".to_string(),
                    function: OllamaFunction {
                        name: tool.name,
                        description: tool.description,
                        parameters: tool.parameters,
                    },
                })
                .collect()
        });

        let request = OllamaRequest {
            model: self.config.model.clone(),
            messages: api_messages,
            stream: true,  // Enable streaming
            tools: api_tools,
            options: OllamaOptions {
                temperature: self.config.temperature,
                num_predict: self.config.max_tokens,
            },
        };

        let url = format!("{}/api/chat", self.base_url);
        debug!("Streaming request to Ollama: model={}", self.config.model);

        let response = self
            .client
            .execute(
                self.client.inner()
                    .post(&url)
                    .json(&request)
            )
            .await?;

        let byte_stream = response.bytes_stream();

        // Ollama streams ndjson: each line is a complete JSON object
        let stream = futures::stream::unfold(
            (byte_stream, String::new()),
            |(mut byte_stream, mut buffer)| async move {
                use futures::StreamExt;
                loop {
                    if let Some(newline_pos) = buffer.find('\n') {
                        let line = buffer[..newline_pos].trim().to_string();
                        buffer = buffer[newline_pos + 1..].to_string();

                        if line.is_empty() {
                            continue;
                        }

                        match serde_json::from_str::<serde_json::Value>(&line) {
                            Ok(json) => {
                                let done = json.get("done")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);

                                let content = json
                                    .get("message")
                                    .and_then(|m| m.get("content"))
                                    .and_then(|c| c.as_str())
                                    .filter(|s| !s.is_empty())
                                    .map(|s| s.to_string());

                                if done {
                                    // Final chunk with optional usage
                                    let usage = json.get("prompt_eval_count").and_then(|p| {
                                        let prompt = p.as_u64()? as u32;
                                        let completion = json.get("eval_count")?.as_u64()? as u32;
                                        Some(crate::provider::TokenUsage {
                                            prompt_tokens: prompt,
                                            completion_tokens: completion,
                                            total_tokens: prompt + completion,
                                        })
                                    });

                                    let chunk = StreamChunk {
                                        delta_content: content,
                                        delta_tool_calls: vec![],
                                        finish_reason: Some(FinishReason::Stop),
                                        usage,
                                    };
                                    return Some((Ok(chunk), (byte_stream, buffer)));
                                }

                                if content.is_some() {
                                    let chunk = StreamChunk {
                                        delta_content: content,
                                        delta_tool_calls: vec![],
                                        finish_reason: None,
                                        usage: None,
                                    };
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

                    match byte_stream.next().await {
                        Some(Ok(bytes)) => buffer.push_str(&String::from_utf8_lossy(&bytes)),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_conversion() {
        assert_eq!(role_to_ollama(&Role::System), "system");
        assert_eq!(role_to_ollama(&Role::User), "user");
        assert_eq!(role_to_ollama(&Role::Assistant), "assistant");
        assert_eq!(role_to_ollama(&Role::Tool), "tool");
    }

    #[test]
    fn test_ollama_response_deserialization() {
        let json = r#"{
            "message": {
                "role": "assistant",
                "content": "thinking",
                "tool_calls": [
                    {
                        "function": {
                            "name": "test_tool",
                            "arguments": {"arg": 1}
                        }
                    }
                ]
            },
            "done": true,
            "prompt_eval_count": 10,
            "eval_count": 20
        }"#;

        let resp: OllamaResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.message.content, "thinking");
        assert_eq!(resp.message.tool_calls.len(), 1);
        assert_eq!(resp.message.tool_calls[0].function.name, "test_tool");
        assert_eq!(resp.prompt_eval_count, 10);
    }

    #[test]
    fn test_provider_name() {
        let config = ProviderConfig::default();
        let provider = LocalProvider::new(config);
        assert_eq!(provider.name(), "ollama");
    }
}
