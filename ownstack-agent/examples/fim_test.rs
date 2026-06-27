//! Validate the OpenAI-compatible FIM backend against codex-everywhere.
use ownstack_agent::fim::{FimBackend, FimConfig, FimEngine};

#[tokio::main]
async fn main() {
    let key = std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY");
    let engine = FimEngine::new(FimConfig {
        backend: FimBackend::OpenAiCompatible,
        model: std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-5.5".into()),
        base_url: "https://codex-everywhere.com/v1".to_string(),
        api_key: key,
        timeout_ms: 25_000, // remote reasoning model is slow for FIM
        ..Default::default()
    });
    let prefix = "def add(a, b):\n    return ";
    let suffix = "\n\nprint(add(2, 3))\n";
    match engine.complete(prefix, suffix).await {
        Ok(mid) => println!("FIM middle = {:?}", mid),
        Err(e) => println!("FIM error: {e}"),
    }
}
