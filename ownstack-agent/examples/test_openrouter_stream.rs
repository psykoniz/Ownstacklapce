use futures::StreamExt;
use ownstack_agent::provider::{
    FinishReason, LlmMessage, LlmProvider, ProviderOptions,
};
use ownstack_agent::providers::openrouter::OpenRouterProvider;
use std::io::Write;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let provider = OpenRouterProvider::from_env()?;
    let model = std::env::var("OPENROUTER_MODEL")
        .unwrap_or_else(|_| "anthropic/claude-3.5-sonnet".to_string());
    let prompt = std::env::var("OWNSTACK_TEST_PROMPT").unwrap_or_else(|_| {
        "Ecris une phrase courte pour valider le streaming.".to_string()
    });

    println!("provider=openrouter");
    println!("model={model}");
    println!("stream=true");
    println!("prompt={prompt}");
    println!("response_stream_start");

    let messages = vec![
        LlmMessage::system("Respond in plain text only."),
        LlmMessage::user(prompt),
    ];

    let started_at = Instant::now();
    let mut first_token_ms: Option<u128> = None;
    let mut final_reason: Option<FinishReason> = None;

    let mut stream = provider
        .stream(messages, None, ProviderOptions::default())
        .await?;
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result?;
        if let Some(delta) = chunk.delta_content {
            if !delta.is_empty() && first_token_ms.is_none() {
                first_token_ms = Some(started_at.elapsed().as_millis());
            }
            print!("{delta}");
            let _ = std::io::stdout().flush();
        }

        if let Some(reason) = chunk.finish_reason {
            final_reason = Some(reason);
        }
    }

    println!();
    println!("response_stream_end");
    println!("ttft_ms={}", first_token_ms.unwrap_or(0));

    match final_reason {
        Some(FinishReason::Stop) => {
            println!("finish_reason=Stop");
            Ok(())
        }
        Some(reason) => {
            Err(format!("unexpected finish reason from stream: {reason:?}").into())
        }
        None => Err("stream ended without finish reason".into()),
    }
}
