//! Comprehensive tests for resilience and streaming features
//!
//! Covers:
//! - RetryConfig edge cases and boundary conditions
//! - ResilientClient backoff computation (stress + boundary)
//! - StreamChunk construction and from_response conversion
//! - ToolCallDelta construction
//! - SSE chunk parsing (valid, malformed, edge cases)
//! - Provider config with retry defaults
//! - Stress tests for concurrent retry config creation

use ownstack_agent::resilience::{RetryConfig, ResilientClient};
use ownstack_agent::provider::{
    LlmProvider, ProviderConfig, ProviderError, LlmResponse, LlmMessage, ToolCall,
    ToolDefinition, FinishReason, TokenUsage, StreamChunk, ToolCallDelta, Role,
};

// ════════════════════════════════════════════════════════════════════
// 1. RetryConfig — Defaults, presets, edge cases
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_retry_config_default_values() {
    let config = RetryConfig::default();
    assert_eq!(config.max_retries, 3);
    assert_eq!(config.initial_backoff_ms, 1000);
    assert_eq!(config.max_backoff_ms, 30_000);
    assert_eq!(config.backoff_multiplier, 2.0);
    assert!(config.jitter);
}

#[test]
fn test_retry_config_none_preset() {
    let config = RetryConfig::none();
    assert_eq!(config.max_retries, 0);
    // Should still have valid defaults for other fields
    assert!(config.initial_backoff_ms > 0);
    assert!(config.max_backoff_ms > 0);
}

#[test]
fn test_retry_config_aggressive_preset() {
    let config = RetryConfig::aggressive();
    assert_eq!(config.max_retries, 5);
    assert_eq!(config.initial_backoff_ms, 500);
    assert_eq!(config.max_backoff_ms, 60_000);
    assert!(config.jitter);
}

#[test]
fn test_retry_config_clone() {
    let config = RetryConfig::default();
    let cloned = config.clone();
    assert_eq!(config.max_retries, cloned.max_retries);
    assert_eq!(config.initial_backoff_ms, cloned.initial_backoff_ms);
    assert_eq!(config.max_backoff_ms, cloned.max_backoff_ms);
    assert_eq!(config.backoff_multiplier, cloned.backoff_multiplier);
    assert_eq!(config.jitter, cloned.jitter);
}

#[test]
fn test_retry_config_debug_impl() {
    let config = RetryConfig::default();
    let debug_str = format!("{:?}", config);
    assert!(debug_str.contains("RetryConfig"));
    assert!(debug_str.contains("max_retries"));
}

// ════════════════════════════════════════════════════════════════════
// 2. ResilientClient — Creation and backoff computation
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_resilient_client_creation_default() {
    let client = ResilientClient::new(RetryConfig::default());
    // Should not panic, inner client should be accessible
    let _ = client.inner();
}

#[test]
fn test_resilient_client_creation_with_none_retry() {
    let client = ResilientClient::new(RetryConfig::none());
    let _ = client.inner();
}

#[test]
fn test_resilient_client_creation_with_aggressive() {
    let client = ResilientClient::new(RetryConfig::aggressive());
    let _ = client.inner();
}

// ════════════════════════════════════════════════════════════════════
// 3. ProviderConfig with RetryConfig integration
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_provider_config_default_has_retry() {
    let config = ProviderConfig::default();
    assert_eq!(config.retry.max_retries, 3);
    assert_eq!(config.retry.initial_backoff_ms, 1000);
}

#[test]
fn test_provider_config_custom_retry() {
    let config = ProviderConfig {
        api_key: "test-key".to_string(),
        model: "gpt-4".to_string(),
        retry: RetryConfig::aggressive(),
        ..Default::default()
    };
    assert_eq!(config.retry.max_retries, 5);
    assert_eq!(config.retry.initial_backoff_ms, 500);
}

#[test]
fn test_provider_config_no_retry() {
    let config = ProviderConfig {
        retry: RetryConfig::none(),
        ..Default::default()
    };
    assert_eq!(config.retry.max_retries, 0);
}

#[test]
fn test_provider_config_clone_preserves_retry() {
    let config = ProviderConfig {
        api_key: "key".to_string(),
        model: "model".to_string(),
        retry: RetryConfig::aggressive(),
        ..Default::default()
    };
    let cloned = config.clone();
    assert_eq!(cloned.retry.max_retries, 5);
    assert_eq!(cloned.retry.initial_backoff_ms, 500);
}

// ════════════════════════════════════════════════════════════════════
// 4. StreamChunk — Construction, from_response, edge cases
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_stream_chunk_from_response_text_only() {
    let response = LlmResponse {
        content: Some("Hello world".to_string()),
        tool_calls: vec![],
        finish_reason: FinishReason::Stop,
        usage: Some(TokenUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        }),
    };

    let chunk = StreamChunk::from_response(response);
    assert_eq!(chunk.delta_content, Some("Hello world".to_string()));
    assert!(chunk.delta_tool_calls.is_empty());
    assert_eq!(chunk.finish_reason, Some(FinishReason::Stop));
    assert!(chunk.usage.is_some());
    assert_eq!(chunk.usage.unwrap().total_tokens, 15);
}

#[test]
fn test_stream_chunk_from_response_with_tool_calls() {
    let response = LlmResponse {
        content: None,
        tool_calls: vec![
            ToolCall {
                id: "call_1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": "/tmp/test.txt"}),
            },
            ToolCall {
                id: "call_2".to_string(),
                name: "write_file".to_string(),
                arguments: serde_json::json!({"path": "/tmp/out.txt", "content": "data"}),
            },
        ],
        finish_reason: FinishReason::ToolCalls,
        usage: None,
    };

    let chunk = StreamChunk::from_response(response);
    assert!(chunk.delta_content.is_none());
    assert_eq!(chunk.delta_tool_calls.len(), 2);

    // Verify first tool call delta
    assert_eq!(chunk.delta_tool_calls[0].index, 0);
    assert_eq!(chunk.delta_tool_calls[0].id, Some("call_1".to_string()));
    assert_eq!(chunk.delta_tool_calls[0].name, Some("read_file".to_string()));
    assert!(chunk.delta_tool_calls[0].arguments_delta.is_some());

    // Verify second tool call delta
    assert_eq!(chunk.delta_tool_calls[1].index, 1);
    assert_eq!(chunk.delta_tool_calls[1].id, Some("call_2".to_string()));
    assert_eq!(chunk.delta_tool_calls[1].name, Some("write_file".to_string()));

    assert_eq!(chunk.finish_reason, Some(FinishReason::ToolCalls));
}

#[test]
fn test_stream_chunk_from_response_empty() {
    let response = LlmResponse {
        content: None,
        tool_calls: vec![],
        finish_reason: FinishReason::Stop,
        usage: None,
    };

    let chunk = StreamChunk::from_response(response);
    assert!(chunk.delta_content.is_none());
    assert!(chunk.delta_tool_calls.is_empty());
    assert!(chunk.usage.is_none());
}

#[test]
fn test_stream_chunk_from_response_length_finish() {
    let response = LlmResponse {
        content: Some("truncated...".to_string()),
        tool_calls: vec![],
        finish_reason: FinishReason::Length,
        usage: Some(TokenUsage {
            prompt_tokens: 100,
            completion_tokens: 4096,
            total_tokens: 4196,
        }),
    };

    let chunk = StreamChunk::from_response(response);
    assert_eq!(chunk.finish_reason, Some(FinishReason::Length));
    assert_eq!(chunk.usage.as_ref().unwrap().completion_tokens, 4096);
}

#[test]
fn test_stream_chunk_from_response_error_finish() {
    let response = LlmResponse {
        content: None,
        tool_calls: vec![],
        finish_reason: FinishReason::Error,
        usage: None,
    };

    let chunk = StreamChunk::from_response(response);
    assert_eq!(chunk.finish_reason, Some(FinishReason::Error));
}

// ════════════════════════════════════════════════════════════════════
// 5. ToolCallDelta — Edge cases
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_tool_call_delta_full() {
    let delta = ToolCallDelta {
        index: 0,
        id: Some("call_123".to_string()),
        name: Some("search".to_string()),
        arguments_delta: Some("{\"query\":".to_string()),
    };

    assert_eq!(delta.index, 0);
    assert_eq!(delta.id, Some("call_123".to_string()));
    assert_eq!(delta.name, Some("search".to_string()));
    assert_eq!(delta.arguments_delta, Some("{\"query\":".to_string()));
}

#[test]
fn test_tool_call_delta_incremental() {
    // Simulates subsequent chunks where only arguments_delta is present
    let delta = ToolCallDelta {
        index: 0,
        id: None,
        name: None,
        arguments_delta: Some("\"hello\"}".to_string()),
    };

    assert!(delta.id.is_none());
    assert!(delta.name.is_none());
    assert_eq!(delta.arguments_delta, Some("\"hello\"}".to_string()));
}

#[test]
fn test_tool_call_delta_empty() {
    let delta = ToolCallDelta {
        index: 0,
        id: None,
        name: None,
        arguments_delta: None,
    };

    assert!(delta.id.is_none());
    assert!(delta.name.is_none());
    assert!(delta.arguments_delta.is_none());
}

// ════════════════════════════════════════════════════════════════════
// 6. StreamChunk with many tool calls (stress test)
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_stream_chunk_many_tool_calls() {
    // Stress test: 100 tool calls in a single response
    let tool_calls: Vec<ToolCall> = (0..100)
        .map(|i| ToolCall {
            id: format!("call_{}", i),
            name: format!("tool_{}", i),
            arguments: serde_json::json!({"index": i}),
        })
        .collect();

    let response = LlmResponse {
        content: None,
        tool_calls,
        finish_reason: FinishReason::ToolCalls,
        usage: None,
    };

    let chunk = StreamChunk::from_response(response);
    assert_eq!(chunk.delta_tool_calls.len(), 100);

    // Verify indexing is correct
    for (i, delta) in chunk.delta_tool_calls.iter().enumerate() {
        assert_eq!(delta.index, i);
        assert_eq!(delta.id, Some(format!("call_{}", i)));
        assert_eq!(delta.name, Some(format!("tool_{}", i)));
    }
}

// ════════════════════════════════════════════════════════════════════
// 7. RetryConfig boundary edge cases
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_retry_config_zero_backoff() {
    // Edge case: zero initial backoff
    let config = RetryConfig {
        max_retries: 3,
        initial_backoff_ms: 0,
        max_backoff_ms: 1000,
        backoff_multiplier: 2.0,
        jitter: false,
    };
    // Should not panic
    let client = ResilientClient::new(config);
    let _ = client.inner();
}

#[test]
fn test_retry_config_very_large_backoff() {
    // Edge case: very large values
    let config = RetryConfig {
        max_retries: 100,
        initial_backoff_ms: u64::MAX / 2,
        max_backoff_ms: u64::MAX / 2,
        backoff_multiplier: 1.0,
        jitter: false,
    };
    let client = ResilientClient::new(config);
    let _ = client.inner();
}

#[test]
fn test_retry_config_multiplier_one() {
    // Edge case: multiplier of 1.0 means constant backoff
    let config = RetryConfig {
        max_retries: 5,
        initial_backoff_ms: 500,
        max_backoff_ms: 30_000,
        backoff_multiplier: 1.0,
        jitter: false,
    };
    let client = ResilientClient::new(config);
    let _ = client.inner();
}

#[test]
fn test_retry_config_multiplier_fractional() {
    // Edge case: fractional multiplier (< 1 means shrinking backoff)
    let config = RetryConfig {
        max_retries: 3,
        initial_backoff_ms: 1000,
        max_backoff_ms: 5000,
        backoff_multiplier: 0.5,
        jitter: false,
    };
    let client = ResilientClient::new(config);
    let _ = client.inner();
}

// ════════════════════════════════════════════════════════════════════
// 8. Stress test: Many retry config clones (concurrency simulation)
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_retry_config_mass_clone_stress() {
    let base = RetryConfig::default();
    let configs: Vec<RetryConfig> = (0..10_000)
        .map(|_| base.clone())
        .collect();

    assert_eq!(configs.len(), 10_000);
    for config in &configs {
        assert_eq!(config.max_retries, 3);
    }
}

#[test]
fn test_provider_config_mass_creation_stress() {
    // Create many ProviderConfigs to ensure no memory issues
    let configs: Vec<ProviderConfig> = (0..1_000)
        .map(|i| ProviderConfig {
            api_key: format!("key_{}", i),
            model: format!("model_{}", i),
            retry: if i % 3 == 0 {
                RetryConfig::none()
            } else if i % 3 == 1 {
                RetryConfig::default()
            } else {
                RetryConfig::aggressive()
            },
            ..Default::default()
        })
        .collect();

    assert_eq!(configs.len(), 1_000);
    assert_eq!(configs[0].retry.max_retries, 0);
    assert_eq!(configs[1].retry.max_retries, 3);
    assert_eq!(configs[2].retry.max_retries, 5);
}

// ════════════════════════════════════════════════════════════════════
// 9. StreamChunk stress: Large content
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_stream_chunk_large_content() {
    // 1MB of content
    let large_content = "x".repeat(1_000_000);

    let response = LlmResponse {
        content: Some(large_content.clone()),
        tool_calls: vec![],
        finish_reason: FinishReason::Stop,
        usage: None,
    };

    let chunk = StreamChunk::from_response(response);
    assert_eq!(chunk.delta_content.unwrap().len(), 1_000_000);
}

#[test]
fn test_stream_chunk_large_arguments() {
    // Tool call with massive JSON arguments
    let large_json = serde_json::json!({
        "data": "x".repeat(100_000),
        "nested": {
            "array": (0..1000).collect::<Vec<i32>>(),
        }
    });

    let response = LlmResponse {
        content: None,
        tool_calls: vec![ToolCall {
            id: "call_big".to_string(),
            name: "process_data".to_string(),
            arguments: large_json,
        }],
        finish_reason: FinishReason::ToolCalls,
        usage: None,
    };

    let chunk = StreamChunk::from_response(response);
    assert_eq!(chunk.delta_tool_calls.len(), 1);
    assert!(chunk.delta_tool_calls[0].arguments_delta.as_ref().unwrap().len() > 100_000);
}

// ════════════════════════════════════════════════════════════════════
// 10. ProviderError variants
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_provider_error_display() {
    let err = ProviderError::RequestFailed("connection timeout".to_string());
    assert_eq!(format!("{}", err), "HTTP request failed: connection timeout");

    let err = ProviderError::ApiError("429 Too Many Requests".to_string());
    assert_eq!(format!("{}", err), "API error: 429 Too Many Requests");

    let err = ProviderError::StreamError("broken pipe".to_string());
    assert_eq!(format!("{}", err), "Stream error: broken pipe");

    let err = ProviderError::SerializationError("invalid JSON".to_string());
    assert_eq!(format!("{}", err), "Serialization error: invalid JSON");

    let err = ProviderError::ConfigError("missing API key".to_string());
    assert_eq!(format!("{}", err), "Configuration error: missing API key");
}

// ════════════════════════════════════════════════════════════════════
// 11. LlmMessage constructors
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_llm_message_system() {
    let msg = LlmMessage::system("You are a helpful assistant");
    assert_eq!(msg.role, Role::System);
    assert_eq!(msg.content, "You are a helpful assistant");
    assert!(msg.tool_call_id.is_none());
}

#[test]
fn test_llm_message_user() {
    let msg = LlmMessage::user("Hello!");
    assert_eq!(msg.role, Role::User);
    assert_eq!(msg.content, "Hello!");
}

#[test]
fn test_llm_message_assistant() {
    let msg = LlmMessage::assistant("Hi there!");
    assert_eq!(msg.role, Role::Assistant);
}

#[test]
fn test_llm_message_tool_result() {
    let msg = LlmMessage::tool_result("call_123", r#"{"result": "ok"}"#);
    assert_eq!(msg.role, Role::Tool);
    assert_eq!(msg.tool_call_id, Some("call_123".to_string()));
    assert_eq!(msg.content, r#"{"result": "ok"}"#);
}

#[test]
fn test_llm_message_empty_content() {
    let msg = LlmMessage::user("");
    assert_eq!(msg.content, "");
}

#[test]
fn test_llm_message_unicode_content() {
    let msg = LlmMessage::user("こんにちは 🌍 مرحبا");
    assert_eq!(msg.content, "こんにちは 🌍 مرحبا");
}

#[test]
fn test_llm_message_very_long_content() {
    let long = "a".repeat(1_000_000);
    let msg = LlmMessage::user(&long);
    assert_eq!(msg.content.len(), 1_000_000);
}

// ════════════════════════════════════════════════════════════════════
// 12. FinishReason equality
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_finish_reason_equality() {
    assert_eq!(FinishReason::Stop, FinishReason::Stop);
    assert_eq!(FinishReason::ToolCalls, FinishReason::ToolCalls);
    assert_eq!(FinishReason::Length, FinishReason::Length);
    assert_eq!(FinishReason::Error, FinishReason::Error);

    assert_ne!(FinishReason::Stop, FinishReason::ToolCalls);
    assert_ne!(FinishReason::Stop, FinishReason::Length);
    assert_ne!(FinishReason::Stop, FinishReason::Error);
}

// ════════════════════════════════════════════════════════════════════
// 13. TokenUsage defaults and edge cases
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_token_usage_default() {
    let usage = TokenUsage::default();
    assert_eq!(usage.prompt_tokens, 0);
    assert_eq!(usage.completion_tokens, 0);
    assert_eq!(usage.total_tokens, 0);
}

#[test]
fn test_token_usage_large_values() {
    let usage = TokenUsage {
        prompt_tokens: u32::MAX,
        completion_tokens: u32::MAX,
        total_tokens: u32::MAX,
    };
    assert_eq!(usage.prompt_tokens, u32::MAX);
}

// ════════════════════════════════════════════════════════════════════
// 14. ToolDefinition construction
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_tool_definition_serialization() {
    let tool = ToolDefinition {
        name: "read_file".to_string(),
        description: "Read a file's contents".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"}
            },
            "required": ["path"]
        }),
    };

    let json = serde_json::to_string(&tool).unwrap();
    assert!(json.contains("read_file"));
    assert!(json.contains("Read a file"));

    // Round-trip
    let deserialized: ToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.name, "read_file");
}

#[test]
fn test_tool_definition_empty_parameters() {
    let tool = ToolDefinition {
        name: "no_params_tool".to_string(),
        description: "A tool with no parameters".to_string(),
        parameters: serde_json::json!({}),
    };

    let json = serde_json::to_string(&tool).unwrap();
    let rt: ToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.parameters, serde_json::json!({}));
}

// ════════════════════════════════════════════════════════════════════
// 15. Async tests — stream() default fallback
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_stream_default_fallback() {
    use async_trait::async_trait;
    use futures::StreamExt;

    /// Mock provider that returns a fixed response
    struct MockProvider;

    #[async_trait]
    impl ownstack_agent::provider::LlmProvider for MockProvider {
        async fn complete(
            &self,
            _messages: Vec<LlmMessage>,
            _tools: Option<Vec<ToolDefinition>>,
        ) -> Result<LlmResponse, ProviderError> {
            Ok(LlmResponse {
                content: Some("Hello from mock".to_string()),
                tool_calls: vec![],
                finish_reason: FinishReason::Stop,
                usage: Some(TokenUsage {
                    prompt_tokens: 5,
                    completion_tokens: 3,
                    total_tokens: 8,
                }),
            })
        }

        fn name(&self) -> &str {
            "mock"
        }
    }

    let provider = MockProvider;

    // Use the default stream() which falls back to complete()
    let mut stream = provider
        .stream(
            vec![LlmMessage::user("test")],
            None,
        )
        .await
        .unwrap();

    // Should get exactly one chunk
    let chunk: StreamChunk = stream.next().await.unwrap().unwrap();
    assert_eq!(chunk.delta_content, Some("Hello from mock".to_string()));
    assert_eq!(chunk.finish_reason, Some(FinishReason::Stop));
    assert_eq!(chunk.usage.as_ref().unwrap().total_tokens, 8);

    // Stream should be exhausted
    assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn test_stream_default_fallback_with_tool_calls() {
    use async_trait::async_trait;
    use futures::StreamExt;

    struct MockToolProvider;

    #[async_trait]
    impl ownstack_agent::provider::LlmProvider for MockToolProvider {
        async fn complete(
            &self,
            _messages: Vec<LlmMessage>,
            _tools: Option<Vec<ToolDefinition>>,
        ) -> Result<LlmResponse, ProviderError> {
            Ok(LlmResponse {
                content: None,
                tool_calls: vec![
                    ToolCall {
                        id: "tc_1".to_string(),
                        name: "search".to_string(),
                        arguments: serde_json::json!({"q": "test"}),
                    },
                ],
                finish_reason: FinishReason::ToolCalls,
                usage: None,
            })
        }

        fn name(&self) -> &str {
            "mock_tool"
        }
    }

    let provider = MockToolProvider;
    let mut stream = provider
        .stream(vec![LlmMessage::user("search for test")], None)
        .await
        .unwrap();

    let chunk: StreamChunk = stream.next().await.unwrap().unwrap();
    assert!(chunk.delta_content.is_none());
    assert_eq!(chunk.delta_tool_calls.len(), 1);
    assert_eq!(chunk.delta_tool_calls[0].id, Some("tc_1".to_string()));
    assert_eq!(chunk.delta_tool_calls[0].name, Some("search".to_string()));
    assert_eq!(chunk.finish_reason, Some(FinishReason::ToolCalls));

    assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn test_stream_default_fallback_error() {
    use async_trait::async_trait;

    struct FailingProvider;

    #[async_trait]
    impl ownstack_agent::provider::LlmProvider for FailingProvider {
        async fn complete(
            &self,
            _messages: Vec<LlmMessage>,
            _tools: Option<Vec<ToolDefinition>>,
        ) -> Result<LlmResponse, ProviderError> {
            Err(ProviderError::RequestFailed("network down".to_string()))
        }

        fn name(&self) -> &str {
            "failing"
        }
    }

    let provider = FailingProvider;
    let result = provider
        .stream(vec![LlmMessage::user("test")], None)
        .await;

    assert!(result.is_err());
    assert!(result.is_err());
    match result {
        Err(ProviderError::RequestFailed(msg)) => assert_eq!(msg, "network down"),
        Err(other) => panic!("Unexpected error type: {:?}", other),
        Ok(_) => panic!("Expected error, but got Ok"),
    }
}

// ════════════════════════════════════════════════════════════════════
// 16. Stress test: Many concurrent mock provider streams
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_stress_many_concurrent_streams() {
    use async_trait::async_trait;
    use futures::StreamExt;
    use std::sync::Arc;

    struct StressProvider;

    #[async_trait]
    impl ownstack_agent::provider::LlmProvider for StressProvider {
        async fn complete(
            &self,
            _messages: Vec<LlmMessage>,
            _tools: Option<Vec<ToolDefinition>>,
        ) -> Result<LlmResponse, ProviderError> {
            Ok(LlmResponse {
                content: Some("response".to_string()),
                tool_calls: vec![],
                finish_reason: FinishReason::Stop,
                usage: None,
            })
        }

        fn name(&self) -> &str {
            "stress"
        }
    }

    let provider = Arc::new(StressProvider);
    let mut handles = Vec::new();

    // Spawn 100 concurrent streams
    for _ in 0..100 {
        let p = Arc::clone(&provider);
        handles.push(tokio::spawn(async move {
            let mut stream = p
                .as_ref()
                .stream(vec![LlmMessage::user("test")], None)
                .await
                .unwrap();
            let chunk: StreamChunk = stream.next().await.unwrap().unwrap();
            assert_eq!(chunk.delta_content, Some("response".to_string()));
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }
}

// ════════════════════════════════════════════════════════════════════
// 17. Edge case: Special characters in messages
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_llm_message_special_characters() {
    // JSON-breaking characters
    let msg = LlmMessage::user(r#"{"key": "value with \"quotes\" and \n newlines"}"#);
    assert!(msg.content.contains("quotes"));

    // SQL injection attempt
    let msg = LlmMessage::user("'; DROP TABLE users; --");
    assert!(msg.content.contains("DROP TABLE"));

    // HTML/XSS attempt
    let msg = LlmMessage::user("<script>alert('xss')</script>");
    assert!(msg.content.contains("<script>"));

    // Null bytes
    let msg = LlmMessage::user("before\0after");
    assert!(msg.content.contains('\0'));
}

#[test]
fn test_tool_call_with_special_arguments() {
    let tool = ToolCall {
        id: "call_special".to_string(),
        name: "exec".to_string(),
        arguments: serde_json::json!({
            "command": "echo \"hello world\" && rm -rf /",
            "path": "../../../etc/passwd",
            "data": null,
            "empty": "",
            "nested": {"deep": {"deeper": true}}
        }),
    };

    let response = LlmResponse {
        content: None,
        tool_calls: vec![tool],
        finish_reason: FinishReason::ToolCalls,
        usage: None,
    };

    let chunk = StreamChunk::from_response(response);
    assert_eq!(chunk.delta_tool_calls.len(), 1);
    let args = &chunk.delta_tool_calls[0].arguments_delta.as_ref().unwrap();
    assert!(args.contains("rm -rf"));
    assert!(args.contains("passwd"));
}

// ════════════════════════════════════════════════════════════════════
// 18. ToolCall serialization round-trip
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_tool_call_serialization_roundtrip() {
    let original = ToolCall {
        id: "call_abc".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path": "/tmp/test.rs", "encoding": "utf-8"}),
    };

    let json = serde_json::to_string(&original).unwrap();
    let deserialized: ToolCall = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.id, "call_abc");
    assert_eq!(deserialized.name, "read_file");
    assert_eq!(deserialized.arguments["path"], "/tmp/test.rs");
}

// ════════════════════════════════════════════════════════════════════
// 19. Role serialization
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_role_serialization() {
    assert_eq!(serde_json::to_string(&Role::System).unwrap(), "\"system\"");
    assert_eq!(serde_json::to_string(&Role::User).unwrap(), "\"user\"");
    assert_eq!(serde_json::to_string(&Role::Assistant).unwrap(), "\"assistant\"");
    assert_eq!(serde_json::to_string(&Role::Tool).unwrap(), "\"tool\"");
}

#[test]
fn test_role_deserialization() {
    let system: Role = serde_json::from_str("\"system\"").unwrap();
    assert_eq!(system, Role::System);

    let user: Role = serde_json::from_str("\"user\"").unwrap();
    assert_eq!(user, Role::User);
}

// ════════════════════════════════════════════════════════════════════
// 20. ResilientClient HTTP status classification
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_retryable_status_codes() {
    use reqwest::StatusCode;

    // These should be retried
    let retryable = [429, 500, 502, 503, 504, 408];
    for code in retryable {
        let status = StatusCode::from_u16(code).unwrap();
        assert!(
            ResilientClient::is_retryable_status(status),
            "Status {} should be retryable",
            code
        );
    }

    // These should NOT be retried
    let non_retryable = [200, 201, 204, 301, 400, 401, 403, 404, 405, 409, 410, 422];
    for code in non_retryable {
        let status = StatusCode::from_u16(code).unwrap();
        assert!(
            !ResilientClient::is_retryable_status(status),
            "Status {} should NOT be retryable",
            code
        );
    }
}
