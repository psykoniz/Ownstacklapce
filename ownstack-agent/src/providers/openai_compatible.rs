//! OpenAI-compatible LLM Provider (configurable base URL, dual wire).
//!
//! Supports two wire formats, selectable via `OPENAI_WIRE_API`:
//!   - `chat`      → POST {base}/v1/chat/completions (OpenAI Chat Completions)
//!   - `responses` → POST {base}/v1/responses        (OpenAI Responses API)
//!
//! Configured entirely from env so it can point at any OpenAI-compatible
//! endpoint (e.g. a self-hosted gateway or a third-party proxy):
//!   OPENAI_BASE_URL, OPENAI_API_KEY, OPENAI_MODEL, OPENAI_WIRE_API.
//!
//! At runtime it probes the configured wire; if the endpoint returns 404 for
//! that path, it transparently retries the other wire and remembers the result,
//! so it works regardless of which API the endpoint actually exposes.

use std::sync::atomic::{AtomicU8, Ordering};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::provider::{
    ContentPart, FinishReason, LlmMessage, LlmProvider, LlmResponse, MessageContent,
    ProviderConfig, ProviderError, ProviderOptions, Role, StreamResult, TokenUsage,
    ToolCall, ToolDefinition,
};
use crate::resilience::ResilientClient;
use crate::secret_store;

const DEFAULT_BASE_URL: &str = "https://api.openai.com";
const DEFAULT_MODEL: &str = "gpt-4o-mini";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Wire {
    Chat,
    Responses,
}

impl Wire {
    fn as_u8(self) -> u8 {
        match self {
            Wire::Chat => 0,
            Wire::Responses => 1,
        }
    }
    fn from_u8(v: u8) -> Self {
        if v == 1 {
            Wire::Responses
        } else {
            Wire::Chat
        }
    }
    fn other(self) -> Self {
        match self {
            Wire::Chat => Wire::Responses,
            Wire::Responses => Wire::Chat,
        }
    }
    fn path(self) -> &'static str {
        match self {
            Wire::Chat => "/v1/chat/completions",
            Wire::Responses => "/v1/responses",
        }
    }
}

pub struct OpenAiCompatibleProvider {
    client: ResilientClient,
    config: ProviderConfig,
    base_url: String,
    /// Effective wire, may flip at runtime after a 404 probe.
    effective_wire: AtomicU8,
}

impl OpenAiCompatibleProvider {
    pub fn new(config: ProviderConfig, wire: &str) -> Self {
        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
            .trim_end_matches('/')
            .to_string();
        let wire = match wire.to_ascii_lowercase().as_str() {
            "responses" => Wire::Responses,
            _ => Wire::Chat,
        };
        let client = ResilientClient::new(config.retry.clone());
        Self {
            client,
            config,
            base_url,
            effective_wire: AtomicU8::new(wire.as_u8()),
        }
    }

    pub fn from_env() -> Result<Self, ProviderError> {
        let api_key = secret_store::get_secret("OPENAI_API_KEY").ok_or_else(|| {
            ProviderError::ConfigError(
                "OPENAI_API_KEY not set (env/keyring)".to_string(),
            )
        })?;
        let base_url = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        let model = std::env::var("OPENAI_MODEL")
            .unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        let wire = std::env::var("OPENAI_WIRE_API")
            .unwrap_or_else(|_| "chat".to_string());

        Ok(Self::new(
            ProviderConfig {
                api_key,
                model,
                base_url: Some(base_url),
                ..Default::default()
            },
            &wire,
        ))
    }

    fn url_for(&self, wire: Wire) -> String {
        format!("{}{}", self.base_url, wire.path())
    }

    async fn complete_with_wire(
        &self,
        wire: Wire,
        messages: &[LlmMessage],
        tools: &Option<Vec<ToolDefinition>>,
        options: &ProviderOptions,
    ) -> Result<LlmResponse, ProviderError> {
        match wire {
            Wire::Chat => self.complete_chat(messages, tools, options).await,
            Wire::Responses => self.complete_responses(messages, tools, options).await,
        }
    }

    // ── Chat Completions wire ────────────────────────────────────────────

    async fn complete_chat(
        &self,
        messages: &[LlmMessage],
        tools: &Option<Vec<ToolDefinition>>,
        options: &ProviderOptions,
    ) -> Result<LlmResponse, ProviderError> {
        let api_messages = to_chat_messages(messages);
        let api_tools = tools.as_ref().map(|t| {
            t.iter()
                .map(|tool| ChatTool {
                    tool_type: "function".to_string(),
                    function: ChatFunction {
                        name: tool.name.clone(),
                        description: tool.description.clone(),
                        parameters: tool.parameters.clone(),
                    },
                })
                .collect::<Vec<_>>()
        });
        let has_tools = api_tools.as_ref().map_or(false, |t| !t.is_empty());

        let request = ChatRequest {
            model: options.model.clone().unwrap_or_else(|| self.config.model.clone()),
            messages: api_messages,
            max_tokens: self.config.max_tokens,
            temperature: self.config.temperature,
            tools: api_tools,
            tool_choice: if has_tools {
                Some(serde_json::Value::String("auto".to_string()))
            } else {
                None
            },
        };

        let response = self
            .client
            .execute(
                self.client
                    .inner()
                    .post(self.url_for(Wire::Chat))
                    .header("Authorization", format!("Bearer {}", self.config.api_key))
                    .json(&request),
            )
            .await?;

        let api: ChatResponse = response
            .json()
            .await
            .map_err(|e| ProviderError::SerializationError(e.to_string()))?;

        let choice = api.choices.into_iter().next().ok_or_else(|| {
            ProviderError::ApiError("No choices in response".to_string())
        })?;

        let tool_calls = parse_chat_tool_calls(choice.message.tool_calls)?;
        let finish_reason = match choice.finish_reason.as_deref() {
            Some("stop") => FinishReason::Stop,
            Some("tool_calls") => FinishReason::ToolCalls,
            Some("length") => FinishReason::Length,
            _ => FinishReason::Stop,
        };
        let usage = api.usage.map(|u| TokenUsage {
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

    // ── Responses API wire ───────────────────────────────────────────────

    async fn complete_responses(
        &self,
        messages: &[LlmMessage],
        tools: &Option<Vec<ToolDefinition>>,
        options: &ProviderOptions,
    ) -> Result<LlmResponse, ProviderError> {
        let input = to_responses_input(messages);
        let api_tools = tools.as_ref().map(|t| {
            t.iter()
                .map(|tool| {
                    serde_json::json!({
                        "type": "function",
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                    })
                })
                .collect::<Vec<_>>()
        });

        let mut body = serde_json::json!({
            "model": options.model.clone().unwrap_or_else(|| self.config.model.clone()),
            "input": input,
            "max_output_tokens": self.config.max_tokens,
            "temperature": self.config.temperature,
        });
        if let Some(t) = api_tools {
            if !t.is_empty() {
                body["tools"] = serde_json::Value::Array(t);
            }
        }

        let response = self
            .client
            .execute(
                self.client
                    .inner()
                    .post(self.url_for(Wire::Responses))
                    .header("Authorization", format!("Bearer {}", self.config.api_key))
                    .json(&body),
            )
            .await?;

        let api: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProviderError::SerializationError(e.to_string()))?;

        parse_responses_output(&api)
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    async fn complete(
        &self,
        messages: Vec<LlmMessage>,
        tools: Option<Vec<ToolDefinition>>,
        options: ProviderOptions,
    ) -> Result<LlmResponse, ProviderError> {
        let wire = Wire::from_u8(self.effective_wire.load(Ordering::Relaxed));
        debug!(
            "OpenAI-compatible: complete via {} ({})",
            match wire {
                Wire::Chat => "chat",
                Wire::Responses => "responses",
            },
            self.base_url
        );

        match self.complete_with_wire(wire, &messages, &tools, &options).await {
            Ok(resp) => Ok(resp),
            // If the configured wire's path is 404, the endpoint likely speaks
            // the other wire — flip, retry once, and remember.
            Err(ProviderError::RequestFailed(ref msg)) if msg.contains("404") => {
                let other = wire.other();
                warn!(
                    "OpenAI-compatible: {} returned 404, retrying with {} wire",
                    wire.path(),
                    other.path()
                );
                let resp = self
                    .complete_with_wire(other, &messages, &tools, &options)
                    .await?;
                self.effective_wire.store(other.as_u8(), Ordering::Relaxed);
                Ok(resp)
            }
            Err(e) => Err(e),
        }
    }

    fn name(&self) -> &str {
        "openai-compatible"
    }

    // Streaming uses the trait's default wrapper around complete(), which is
    // correct for both wires; native SSE can be added later if needed.
    fn supports_streaming(&self) -> bool {
        false
    }

    async fn stream(
        &self,
        messages: Vec<LlmMessage>,
        tools: Option<Vec<ToolDefinition>>,
        options: ProviderOptions,
    ) -> Result<StreamResult, ProviderError> {
        use crate::provider::StreamChunk;
        let response = self.complete(messages, tools, options).await?;
        let chunk = StreamChunk::from_response(response);
        Ok(Box::pin(futures::stream::once(async move { Ok(chunk) })))
    }
}

// ─── Chat Completions wire types ──────────────────────────────────────────

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    max_tokens: u32,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ChatTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ChatToolCallReq>>,
}

#[derive(Serialize)]
struct ChatTool {
    #[serde(rename = "type")]
    tool_type: String,
    function: ChatFunction,
}

#[derive(Serialize)]
struct ChatFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Serialize)]
struct ChatToolCallReq {
    id: String,
    #[serde(rename = "type")]
    tool_type: String,
    function: ChatFunctionCallReq,
}

#[derive(Serialize)]
struct ChatFunctionCallReq {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
    usage: Option<ChatUsage>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatRespMessage,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct ChatRespMessage {
    content: Option<String>,
    tool_calls: Option<Vec<ChatRespToolCall>>,
}

#[derive(Deserialize)]
struct ChatRespToolCall {
    id: String,
    function: ChatRespFunctionCall,
}

#[derive(Deserialize)]
struct ChatRespFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct ChatUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

fn role_str(role: &Role) -> String {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
    .to_string()
}

fn content_to_chat_value(content: &MessageContent) -> serde_json::Value {
    match content {
        MessageContent::Text(s) => serde_json::Value::String(s.clone()),
        MessageContent::Parts(parts) => {
            let arr = parts
                .iter()
                .map(|p| match p {
                    ContentPart::Text { text } => {
                        serde_json::json!({"type": "text", "text": text})
                    }
                    ContentPart::Image { source } => serde_json::json!({
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:{};base64,{}", source.media_type, source.data)
                        }
                    }),
                })
                .collect::<Vec<_>>();
            serde_json::Value::Array(arr)
        }
    }
}

fn to_chat_messages(messages: &[LlmMessage]) -> Vec<ChatMessage> {
    messages
        .iter()
        .map(|m| {
            let tool_calls = m.tool_calls.as_ref().map(|calls| {
                calls
                    .iter()
                    .map(|call| ChatToolCallReq {
                        id: call.id.clone(),
                        tool_type: "function".to_string(),
                        function: ChatFunctionCallReq {
                            name: call.name.clone(),
                            arguments: call.arguments.to_string(),
                        },
                    })
                    .collect()
            });
            let content = if tool_calls.is_some() {
                None
            } else {
                Some(content_to_chat_value(&m.content))
            };
            ChatMessage {
                role: role_str(&m.role),
                content,
                tool_call_id: m.tool_call_id.clone(),
                tool_calls,
            }
        })
        .collect()
}

fn parse_chat_tool_calls(
    tool_calls: Option<Vec<ChatRespToolCall>>,
) -> Result<Vec<ToolCall>, ProviderError> {
    tool_calls
        .unwrap_or_default()
        .into_iter()
        .map(|tc| {
            let arguments = serde_json::from_str::<serde_json::Value>(
                &tc.function.arguments,
            )
            .map_err(|e| {
                ProviderError::SerializationError(format!(
                    "tool-call args parse error for '{}': {}",
                    tc.function.name, e
                ))
            })?;
            Ok(ToolCall {
                id: tc.id,
                name: tc.function.name,
                arguments,
            })
        })
        .collect()
}

// ─── Responses wire helpers ───────────────────────────────────────────────

/// Convert messages to the Responses API `input` array.
fn to_responses_input(messages: &[LlmMessage]) -> Vec<serde_json::Value> {
    messages
        .iter()
        .map(|m| {
            let text = m.content.get_text();
            // Responses uses input_text for user/system, output_text for assistant.
            let part_type = match m.role {
                Role::Assistant => "output_text",
                _ => "input_text",
            };
            serde_json::json!({
                "role": role_str(&m.role),
                "content": [ { "type": part_type, "text": text } ],
            })
        })
        .collect()
}

/// Parse a Responses API result into an LlmResponse. Tolerant of both the
/// structured `output` array and a flattened `output_text` convenience field.
fn parse_responses_output(api: &serde_json::Value) -> Result<LlmResponse, ProviderError> {
    let mut content = String::new();
    let mut tool_calls = Vec::new();

    if let Some(output) = api.get("output").and_then(|v| v.as_array()) {
        for item in output {
            match item.get("type").and_then(|t| t.as_str()) {
                Some("message") => {
                    if let Some(parts) = item.get("content").and_then(|c| c.as_array()) {
                        for part in parts {
                            if let Some(t) =
                                part.get("text").and_then(|t| t.as_str())
                            {
                                content.push_str(t);
                            }
                        }
                    }
                }
                Some("function_call") => {
                    let name = item
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let id = item
                        .get("call_id")
                        .or_else(|| item.get("id"))
                        .and_then(|i| i.as_str())
                        .unwrap_or("call_0")
                        .to_string();
                    let raw_args = item
                        .get("arguments")
                        .and_then(|a| a.as_str())
                        .unwrap_or("{}");
                    let arguments =
                        serde_json::from_str::<serde_json::Value>(raw_args)
                            .unwrap_or(serde_json::json!({}));
                    tool_calls.push(ToolCall { id, name, arguments });
                }
                _ => {}
            }
        }
    }

    // Fallback: some gateways expose a flattened `output_text`.
    if content.is_empty() && tool_calls.is_empty() {
        if let Some(t) = api.get("output_text").and_then(|t| t.as_str()) {
            content.push_str(t);
        }
    }

    let usage = api.get("usage").map(|u| TokenUsage {
        prompt_tokens: u
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        completion_tokens: u
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        total_tokens: u
            .get("total_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
    });

    let finish_reason = if !tool_calls.is_empty() {
        FinishReason::ToolCalls
    } else {
        FinishReason::Stop
    };

    Ok(LlmResponse {
        content: if content.is_empty() { None } else { Some(content) },
        tool_calls,
        finish_reason,
        usage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_responses_structured_output() {
        let api = serde_json::json!({
            "output": [
                {"type": "message", "role": "assistant",
                 "content": [{"type": "output_text", "text": "Hello"}]}
            ],
            "usage": {"input_tokens": 5, "output_tokens": 1, "total_tokens": 6}
        });
        let resp = parse_responses_output(&api).unwrap();
        assert_eq!(resp.content.as_deref(), Some("Hello"));
        assert_eq!(resp.finish_reason, FinishReason::Stop);
        assert_eq!(resp.usage.unwrap().total_tokens, 6);
    }

    #[test]
    fn parses_responses_function_call() {
        let api = serde_json::json!({
            "output": [
                {"type": "function_call", "name": "search",
                 "call_id": "c1", "arguments": "{\"q\":\"x\"}"}
            ]
        });
        let resp = parse_responses_output(&api).unwrap();
        assert_eq!(resp.finish_reason, FinishReason::ToolCalls);
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].name, "search");
        assert_eq!(resp.tool_calls[0].arguments["q"], "x");
    }

    #[test]
    fn parses_responses_flattened_output_text() {
        let api = serde_json::json!({"output_text": "Flat"});
        let resp = parse_responses_output(&api).unwrap();
        assert_eq!(resp.content.as_deref(), Some("Flat"));
    }

    #[test]
    fn wire_selection_defaults_to_chat() {
        let p = OpenAiCompatibleProvider::new(
            ProviderConfig {
                api_key: "k".to_string(),
                model: "m".to_string(),
                base_url: Some("https://example.com/".to_string()),
                ..Default::default()
            },
            "unknown",
        );
        assert_eq!(p.base_url, "https://example.com");
        assert_eq!(p.url_for(Wire::Chat), "https://example.com/v1/chat/completions");
        assert_eq!(p.url_for(Wire::Responses), "https://example.com/v1/responses");
    }
}
