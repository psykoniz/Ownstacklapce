use ownstack_agent::provider::{
    LlmMessage, LlmProvider, ProviderError, ProviderOptions,
};
use ownstack_agent::providers::openrouter::OpenRouterProvider;

fn classify_error(err: &ProviderError) -> &'static str {
    match err {
        ProviderError::ConfigError(_) => "configuration_error",
        ProviderError::SerializationError(_) => "serialization_error",
        ProviderError::StreamError(_) => "stream_error",
        ProviderError::RequestFailed(msg) => {
            let msg = msg.to_ascii_lowercase();
            if msg.contains("timeout") {
                "timeout"
            } else if msg.contains("connection")
                || msg.contains("dns")
                || msg.contains("network")
            {
                "network_error"
            } else {
                "request_failed"
            }
        }
        ProviderError::ApiError(msg) => {
            let lower = msg.to_ascii_lowercase();
            if msg.contains(" 429") || msg.contains("HTTP 429") {
                "rate_limited"
            } else if msg.contains(" 401")
                || msg.contains(" 403")
                || lower.contains("unauthorized")
            {
                "auth_error"
            } else if msg.contains(" 404")
                || msg.contains(" 422")
                || lower.contains("valid model")
                || lower.contains("invalid model")
            {
                "invalid_model_or_request"
            } else {
                "api_error"
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let provider = OpenRouterProvider::from_env()?;
    let model = std::env::var("OPENROUTER_MODEL")
        .unwrap_or_else(|_| "anthropic/claude-3.5-sonnet".to_string());
    let prompt = std::env::var("OWNSTACK_TEST_PROMPT")
        .unwrap_or_else(|_| "Dis bonjour en une phrase.".to_string());

    println!("provider=openrouter");
    println!("model={model}");
    println!("prompt={prompt}");

    let messages = vec![
        LlmMessage::system("Respond in plain text only."),
        LlmMessage::user(prompt),
    ];

    let response = match provider
        .complete(messages, None, ProviderOptions::default())
        .await
    {
        Ok(r) => r,
        Err(err) => {
            eprintln!("error_class={}", classify_error(&err));
            eprintln!("error={err}");
            return Err(Box::<dyn std::error::Error>::from(err));
        }
    };

    let content = response.content.unwrap_or_default();
    if content.trim().is_empty() {
        return Err("OpenRouter returned empty content".into());
    }

    println!("finish_reason={:?}", response.finish_reason);
    if let Some(usage) = response.usage {
        println!(
            "usage_prompt={} usage_completion={} usage_total={}",
            usage.prompt_tokens, usage.completion_tokens, usage.total_tokens
        );
    } else {
        println!("usage=none");
    }
    println!("response={content}");
    Ok(())
}
