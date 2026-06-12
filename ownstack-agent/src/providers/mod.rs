//! LLM Provider implementations

pub mod anthropic;
pub mod local;
pub mod openai_compatible;
pub mod openrouter;

pub use anthropic::AnthropicProvider;
pub use local::LocalProvider;
pub use openai_compatible::OpenAiCompatibleProvider;
pub use openrouter::OpenRouterProvider;
