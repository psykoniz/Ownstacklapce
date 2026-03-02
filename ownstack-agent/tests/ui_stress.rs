use async_trait::async_trait;
use futures::stream;
use ownstack_agent::provider::{
    FinishReason, LlmMessage, LlmProvider, LlmResponse, ProviderError, StreamChunk,
    StreamResult, ToolDefinition,
};
use std::sync::Arc;
use std::time::Duration;

pub struct MockFloodProvider {
    pub chunk_count: usize,
    pub delay_ms: u64,
}

#[async_trait]
impl LlmProvider for MockFloodProvider {
    async fn complete(
        &self,
        _messages: Vec<LlmMessage>,
        _tools: Option<Vec<ToolDefinition>>,
        _model_override: Option<String>,
    ) -> Result<LlmResponse, ProviderError> {
        Ok(LlmResponse {
            content: Some("done".to_string()),
            tool_calls: vec![],
            finish_reason: FinishReason::Stop,
            usage: None,
        })
    }

    async fn stream(
        &self,
        _messages: Vec<LlmMessage>,
        _tools: Option<Vec<ToolDefinition>>,
        _model_override: Option<String>,
    ) -> Result<StreamResult, ProviderError> {
        let count = self.chunk_count;
        let delay = self.delay_ms;

        let s = stream::unfold(0, move |state| async move {
            if state >= count {
                return None;
            }
            if delay > 0 {
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
            let chunk = StreamChunk {
                delta_content: Some(format!("chunk {} ", state)),
                delta_tool_calls: vec![],
                finish_reason: if state == count - 1 {
                    Some(FinishReason::Stop)
                } else {
                    None
                },
                usage: None,
            };
            Some((Ok(chunk), state + 1))
        });

        Ok(Box::pin(s))
    }

    fn name(&self) -> &str {
        "flood_mock"
    }
    fn supports_streaming(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ownstack_agent::orchestrator::AgentOrchestrator;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_high_frequency_flood() {
        let provider = Arc::new(MockFloodProvider {
            chunk_count: 500,
            delay_ms: 0,
        });
        let mut orchestrator =
            AgentOrchestrator::new(provider, PathBuf::from("."), 1024);

        let mut received = 0;
        let result = orchestrator
            .stream_process(
                "test",
                |_| {
                    received += 1;
                },
                |_| {},
                |_, _| {},
            )
            .await;

        assert!(result.is_ok());
        assert_eq!(received, 500);
    }

    #[tokio::test]
    async fn test_low_frequency_burst() {
        let provider = Arc::new(MockFloodProvider {
            chunk_count: 50,
            delay_ms: 2,
        });
        let mut orchestrator =
            AgentOrchestrator::new(provider, PathBuf::from("."), 1024);

        let mut received = 0;
        let _ = orchestrator
            .stream_process(
                "test",
                |_| {
                    received += 1;
                },
                |_| {},
                |_, _| {},
            )
            .await;

        assert_eq!(received, 50);
    }
}
