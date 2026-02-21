//! LLM Provider trait and common types

use async_trait::async_trait;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use thiserror::Error;

use crate::resilience::RetryConfig;

/// Errors that can occur during LLM operations
#[derive(Error, Debug)]
pub enum ProviderError {
    #[error("HTTP request failed: {0}")]
    RequestFailed(String),
    #[error("API error: {0}")]
    ApiError(String),
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Stream error: {0}")]
    StreamError(String),
    #[error("Configuration error: {0}")]
    ConfigError(String),
}

/// Message role in a conversation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// A message in the conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: Role,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

impl LlmMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            tool_call_id: None,
            tool_calls: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            tool_call_id: None,
            tool_calls: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            tool_call_id: None,
            tool_calls: None,
        }
    }

    pub fn tool_result(
        tool_call_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            tool_call_id: Some(tool_call_id.into()),
            tool_calls: None,
        }
    }
}

/// A tool call requested by the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Tool definition for the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Response from the LLM
#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: FinishReason,
    pub usage: Option<TokenUsage>,
}

/// Why the LLM stopped generating
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FinishReason {
    Stop,
    ToolCalls,
    Length,
    Error,
}

/// Token usage statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

// ─── Streaming Types ───────────────────────────────────────────────

/// A single chunk from a streaming LLM response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    /// Incremental text content delta
    pub delta_content: Option<String>,
    /// Incremental tool call deltas
    pub delta_tool_calls: Vec<ToolCallDelta>,
    /// Set when the stream is complete
    pub finish_reason: Option<FinishReason>,
    /// Token usage (typically only in the final chunk)
    pub usage: Option<TokenUsage>,
}

/// Incremental update for a tool call being streamed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallDelta {
    /// Index of the tool call in the array
    pub index: usize,
    /// Tool call ID (only present in the first delta)
    pub id: Option<String>,
    /// Function name (only present in the first delta)
    pub name: Option<String>,
    /// Incremental arguments JSON fragment
    pub arguments_delta: Option<String>,
}

impl StreamChunk {
    /// Create a StreamChunk from a complete LlmResponse (for fallback)
    pub fn from_response(response: LlmResponse) -> Self {
        let delta_tool_calls = response
            .tool_calls
            .into_iter()
            .enumerate()
            .map(|(i, tc)| ToolCallDelta {
                index: i,
                id: Some(tc.id),
                name: Some(tc.name),
                arguments_delta: Some(tc.arguments.to_string()),
            })
            .collect();

        Self {
            delta_content: response.content,
            delta_tool_calls,
            finish_reason: Some(response.finish_reason),
            usage: response.usage,
        }
    }
}

/// Type alias for streamed responses
pub type StreamResult =
    Pin<Box<dyn Stream<Item = Result<StreamChunk, ProviderError>> + Send>>;

/// Configuration for LLM providers
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub api_key: String,
    pub model: String,
    pub base_url: Option<String>,
    pub max_tokens: u32,
    pub temperature: f32,
    /// Retry configuration for transient failures
    pub retry: RetryConfig,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            model: "claude-3-5-sonnet-20241022".to_string(),
            base_url: None,
            max_tokens: 4096,
            temperature: 0.7,
            retry: RetryConfig::default(),
        }
    }
}

/// Trait for LLM providers
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Complete a conversation with the LLM (non-streaming)
    async fn complete(
        &self,
        messages: Vec<LlmMessage>,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<LlmResponse, ProviderError>;

    /// Stream a conversation with the LLM
    ///
    /// Default implementation falls back to `complete()` and wraps
    /// the result as a single StreamChunk. Providers that support
    /// native streaming should override this method.
    async fn stream(
        &self,
        messages: Vec<LlmMessage>,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<StreamResult, ProviderError> {
        let response = self.complete(messages, tools).await?;
        let chunk = StreamChunk::from_response(response);
        Ok(Box::pin(futures::stream::once(async move { Ok(chunk) })))
    }

    /// Get the provider name
    fn name(&self) -> &str;

    /// Check if provider supports native streaming
    fn supports_streaming(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── LlmMessage Constructors ─────────────────────────────────
    #[test]
    fn test_system_message() {
        let m = LlmMessage::system("system prompt");
        assert_eq!(m.role, Role::System);
        assert_eq!(m.content, "system prompt");
        assert!(m.tool_call_id.is_none());
        assert!(m.tool_calls.is_none());
    }

    #[test]
    fn test_user_message() {
        let m = LlmMessage::user("user query");
        assert_eq!(m.role, Role::User);
        assert_eq!(m.content, "user query");
    }

    #[test]
    fn test_assistant_message() {
        let m = LlmMessage::assistant("response");
        assert_eq!(m.role, Role::Assistant);
        assert_eq!(m.content, "response");
    }

    #[test]
    fn test_tool_result_message() {
        let m = LlmMessage::tool_result("call_1", "tool output");
        assert_eq!(m.role, Role::Tool);
        assert_eq!(m.content, "tool output");
        assert_eq!(m.tool_call_id, Some("call_1".to_string()));
    }

    #[test]
    fn test_message_with_empty_content() {
        let m = LlmMessage::user("");
        assert_eq!(m.content, "");
    }

    #[test]
    fn test_message_with_unicode() {
        let m = LlmMessage::user("日本語 🦀 émojis");
        assert!(m.content.contains("🦀"));
    }

    #[test]
    fn test_message_with_very_long_content() {
        let content = "x".repeat(100_000);
        let m = LlmMessage::user(content.clone());
        assert_eq!(m.content.len(), 100_000);
    }

    // ─── Role Serialization ──────────────────────────────────────
    #[test]
    fn test_role_serialization() {
        let roles = [Role::System, Role::User, Role::Assistant, Role::Tool];
        let expected = ["system", "user", "assistant", "tool"];
        for (role, exp) in roles.iter().zip(expected.iter()) {
            let json = serde_json::to_string(role).unwrap();
            assert!(
                json.contains(exp),
                "Role {:?} should serialize to {}",
                role,
                exp
            );
        }
    }

    #[test]
    fn test_role_deserialization_roundtrip() {
        for role in [Role::System, Role::User, Role::Assistant, Role::Tool] {
            let json = serde_json::to_string(&role).unwrap();
            let back: Role = serde_json::from_str(&json).unwrap();
            assert_eq!(role, back);
        }
    }

    // ─── LlmMessage Serialization ────────────────────────────────
    #[test]
    fn test_message_serialize_roundtrip() {
        let m = LlmMessage::user("Hello!");
        let json = serde_json::to_string(&m).unwrap();
        let back: LlmMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.role, Role::User);
        assert_eq!(back.content, "Hello!");
    }

    #[test]
    fn test_message_serialize_tool_call_id_none() {
        let m = LlmMessage::user("test");
        let json = serde_json::to_string(&m).unwrap();
        // tool_call_id should be skipped when None
        assert!(!json.contains("tool_calls") || json.contains("null"));
    }

    #[test]
    fn test_message_serialize_with_tool_calls() {
        let mut m = LlmMessage::assistant("I need to call tools");
        m.tool_calls = Some(vec![ToolCall {
            id: "call_1".to_string(),
            name: "exec".to_string(),
            arguments: serde_json::json!({"command": "ls"}),
        }]);
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("call_1"));
        assert!(json.contains("exec"));
    }

    // ─── ToolCall ────────────────────────────────────────────────
    #[test]
    fn test_tool_call_creation() {
        let tc = ToolCall {
            id: "id_123".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "src/main.rs"}),
        };
        assert_eq!(tc.id, "id_123");
        assert_eq!(tc.name, "read_file");
    }

    #[test]
    fn test_tool_call_serialize_roundtrip() {
        let tc = ToolCall {
            id: "tc_1".to_string(),
            name: "exec".to_string(),
            arguments: serde_json::json!({"cmd": "cargo build"}),
        };
        let json = serde_json::to_string(&tc).unwrap();
        let back: ToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "tc_1");
        assert_eq!(back.name, "exec");
    }

    // ─── ToolDefinition ──────────────────────────────────────────
    #[test]
    fn test_tool_definition() {
        let td = ToolDefinition {
            name: "search".to_string(),
            description: "Search files".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        };
        assert_eq!(td.name, "search");
    }

    #[test]
    fn test_tool_definition_serialize() {
        let td = ToolDefinition {
            name: "test".to_string(),
            description: "A test".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        };
        let json = serde_json::to_string(&td).unwrap();
        assert!(json.contains("test"));
    }

    // ─── ProviderConfig ──────────────────────────────────────────
    #[test]
    fn test_config_default() {
        let cfg = ProviderConfig::default();
        assert!(cfg.api_key.is_empty());
        assert_eq!(cfg.model, "claude-3-5-sonnet-20241022");
        assert!(cfg.base_url.is_none());
        assert_eq!(cfg.max_tokens, 4096);
        assert!((cfg.temperature - 0.7).abs() < 0.001);
    }

    #[test]
    fn test_config_custom() {
        let cfg = ProviderConfig {
            api_key: "test-key".to_string(),
            model: "gpt-4".to_string(),
            base_url: Some("https://api.example.com".to_string()),
            max_tokens: 8192,
            temperature: 0.0,
            retry: RetryConfig::default(),
        };
        assert_eq!(cfg.api_key, "test-key");
        assert_eq!(cfg.model, "gpt-4");
        assert_eq!(cfg.max_tokens, 8192);
    }

    // ─── ProviderError ───────────────────────────────────────────
    #[test]
    fn test_error_display() {
        let e = ProviderError::RequestFailed("timeout".to_string());
        assert!(e.to_string().contains("timeout"));

        let e2 = ProviderError::ApiError("rate limited".to_string());
        assert!(e2.to_string().contains("rate limited"));

        let e3 = ProviderError::ConfigError("missing key".to_string());
        assert!(e3.to_string().contains("missing key"));
    }

    // ─── FinishReason ────────────────────────────────────────────
    #[test]
    fn test_finish_reason_eq() {
        assert_eq!(FinishReason::Stop, FinishReason::Stop);
        assert_ne!(FinishReason::Stop, FinishReason::ToolCalls);
        assert_ne!(FinishReason::Length, FinishReason::Error);
    }

    // ─── TokenUsage ──────────────────────────────────────────────
    #[test]
    fn test_token_usage_default() {
        let u = TokenUsage::default();
        assert_eq!(u.prompt_tokens, 0);
        assert_eq!(u.completion_tokens, 0);
        assert_eq!(u.total_tokens, 0);
    }

    #[test]
    fn test_token_usage_custom() {
        let u = TokenUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        };
        assert_eq!(u.total_tokens, 150);
    }

    // ─── LlmResponse ────────────────────────────────────────────
    #[test]
    fn test_llm_response_with_content() {
        let r = LlmResponse {
            content: Some("response text".to_string()),
            tool_calls: vec![],
            finish_reason: FinishReason::Stop,
            usage: None,
        };
        assert_eq!(r.content.unwrap(), "response text");
        assert!(r.tool_calls.is_empty());
    }

    #[test]
    fn test_llm_response_with_tool_calls() {
        let r = LlmResponse {
            content: None,
            tool_calls: vec![ToolCall {
                id: "tc1".to_string(),
                name: "exec".to_string(),
                arguments: serde_json::json!({}),
            }],
            finish_reason: FinishReason::ToolCalls,
            usage: Some(TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
        };
        assert!(r.content.is_none());
        assert_eq!(r.tool_calls.len(), 1);
        assert_eq!(r.finish_reason, FinishReason::ToolCalls);
    }

    // ─── Clone Tests ─────────────────────────────────────────────
    #[test]
    fn test_message_clone() {
        let m = LlmMessage::user("test");
        let m2 = m.clone();
        assert_eq!(m.content, m2.content);
        assert_eq!(m.role, m2.role);
    }

    #[test]
    fn test_config_clone() {
        let cfg = ProviderConfig::default();
        let cfg2 = cfg.clone();
        assert_eq!(cfg.model, cfg2.model);
    }

    // ─── Stress Tests ────────────────────────────────────────────
    #[test]
    fn stress_test_1000_message_creations() {
        for i in 0..1000 {
            let m = match i % 4 {
                0 => LlmMessage::system(format!("sys_{}", i)),
                1 => LlmMessage::user(format!("usr_{}", i)),
                2 => LlmMessage::assistant(format!("asst_{}", i)),
                _ => LlmMessage::tool_result(
                    format!("tc_{}", i),
                    format!("res_{}", i),
                ),
            };
            let json = serde_json::to_string(&m).unwrap();
            let _: LlmMessage = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn stress_test_concurrent_message_creation() {
        use std::thread;
        let handles: Vec<_> = (0..50)
            .map(|i| {
                thread::spawn(move || {
                    for j in 0..100 {
                        let m = LlmMessage::user(format!("msg_{}_{}", i, j));
                        let _ = serde_json::to_string(&m).unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
    }
}
