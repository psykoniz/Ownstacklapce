//! Fill-in-the-Middle (FIM) code completion.
//!
//! Powers OwnStack's inline "ghost text" autocompletion. Unlike the chat
//! orchestrator, FIM is stateless and latency-critical: a request carries the
//! code *before* the cursor (prefix) and *after* the cursor (suffix), and the
//! model returns the text that belongs in the middle.
//!
//! Two backends are supported:
//! - **Ollama** (`/api/generate` with `raw: true`) for local, private, fast
//!   completion using code models like Qwen2.5-Coder or DeepSeek-Coder.
//! - **OpenRouter** (chat completions) as a network fallback for users without
//!   a local GPU.
//!
//! The FIM token format differs per model family, so we detect the family from
//! the model name and pick the right template.

use serde::Deserialize;
use std::time::Duration;

/// Which backend serves FIM completions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FimBackend {
    /// Local Ollama instance.
    Ollama,
    /// OpenRouter network API.
    OpenRouter,
}

/// Configuration for a FIM completion engine.
#[derive(Debug, Clone)]
pub struct FimConfig {
    pub backend: FimBackend,
    /// Model identifier (e.g. `qwen2.5-coder:1.5b`, `deepseek/deepseek-coder`).
    pub model: String,
    /// Base URL for the backend (Ollama host or OpenRouter endpoint).
    pub base_url: String,
    /// API key (only used by OpenRouter).
    pub api_key: String,
    /// Maximum tokens to generate per completion.
    pub max_tokens: u32,
    /// Sampling temperature (low for deterministic code).
    pub temperature: f32,
    /// Hard timeout — if a completion takes longer it is abandoned so the
    /// editor never stalls.
    pub timeout_ms: u64,
}

impl Default for FimConfig {
    fn default() -> Self {
        Self {
            backend: FimBackend::Ollama,
            model: "qwen2.5-coder:1.5b".to_string(),
            base_url: "http://localhost:11434".to_string(),
            api_key: String::new(),
            max_tokens: 128,
            temperature: 0.1,
            timeout_ms: 800,
        }
    }
}

/// FIM token templates for the major code-model families.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FimFamily {
    Qwen,
    DeepSeek,
    StarCoder,
    CodeLlama,
}

impl FimFamily {
    /// Detect the model family from its name (case-insensitive substring match).
    fn detect(model: &str) -> Self {
        let m = model.to_ascii_lowercase();
        if m.contains("deepseek") {
            FimFamily::DeepSeek
        } else if m.contains("starcoder") || m.contains("stable-code") {
            FimFamily::StarCoder
        } else if m.contains("codellama") || m.contains("code-llama") {
            FimFamily::CodeLlama
        } else {
            // Qwen2.5-Coder and most modern coders use the Qwen/PSM format.
            FimFamily::Qwen
        }
    }

    /// Build the raw FIM prompt for this family.
    ///
    /// All supported families use the Prefix-Suffix-Middle (PSM) ordering,
    /// only the sentinel tokens differ.
    fn build_prompt(&self, prefix: &str, suffix: &str) -> String {
        match self {
            FimFamily::Qwen => format!(
                "<|fim_prefix|>{prefix}<|fim_suffix|>{suffix}<|fim_middle|>"
            ),
            FimFamily::DeepSeek => format!(
                "<｜fim▁begin｜>{prefix}<｜fim▁hole｜>{suffix}<｜fim▁end｜>"
            ),
            FimFamily::StarCoder => {
                format!("<fim_prefix>{prefix}<fim_suffix>{suffix}<fim_middle>")
            }
            FimFamily::CodeLlama => {
                format!("<PRE> {prefix} <SUF>{suffix} <MID>")
            }
        }
    }

    /// Stop sequences that mark the end of a useful completion.
    fn stop_tokens(&self) -> Vec<String> {
        let mut common = vec![
            "<|endoftext|>".to_string(),
            "<|file_separator|>".to_string(),
        ];
        match self {
            FimFamily::Qwen => {
                common.push("<|fim_pad|>".to_string());
                common.push("<|repo_name|>".to_string());
            }
            FimFamily::DeepSeek => common.push("<｜end▁of▁sentence｜>".to_string()),
            FimFamily::StarCoder => common.push("<|end_of_text|>".to_string()),
            FimFamily::CodeLlama => common.push("<EOT>".to_string()),
        }
        common
    }
}

/// A FIM completion engine bound to one backend configuration.
pub struct FimEngine {
    config: FimConfig,
    client: reqwest::Client,
}

impl FimEngine {
    pub fn new(config: FimConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(config.timeout_ms))
            .build()
            .unwrap_or_default();
        Self { config, client }
    }

    pub fn config(&self) -> &FimConfig {
        &self.config
    }

    /// Produce a completion for the given prefix/suffix. Returns `Ok(String)`
    /// with the middle text (possibly empty) or an error describing the
    /// failure. Callers should treat errors as "no suggestion" and stay silent.
    pub async fn complete(
        &self,
        prefix: &str,
        suffix: &str,
    ) -> Result<String, String> {
        match self.config.backend {
            FimBackend::Ollama => self.complete_ollama(prefix, suffix).await,
            FimBackend::OpenRouter => {
                self.complete_openrouter(prefix, suffix).await
            }
        }
    }

    async fn complete_ollama(
        &self,
        prefix: &str,
        suffix: &str,
    ) -> Result<String, String> {
        let family = FimFamily::detect(&self.config.model);
        let raw_prompt = family.build_prompt(prefix, suffix);

        let body = serde_json::json!({
            "model": self.config.model,
            "prompt": raw_prompt,
            "raw": true,
            "stream": false,
            "options": {
                "temperature": self.config.temperature,
                "num_predict": self.config.max_tokens,
                "stop": family.stop_tokens(),
            }
        });

        let url = format!("{}/api/generate", self.config.base_url);
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("ollama request failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("ollama status {}", resp.status()));
        }

        let parsed: OllamaGenerateResponse = resp
            .json()
            .await
            .map_err(|e| format!("ollama decode failed: {e}"))?;

        Ok(clean_completion(&parsed.response))
    }

    async fn complete_openrouter(
        &self,
        prefix: &str,
        suffix: &str,
    ) -> Result<String, String> {
        // OpenRouter has no raw FIM endpoint; we instruct a chat model to act
        // as a completion engine and return ONLY the middle text.
        let system = "You are a code completion engine. Given CODE_BEFORE and \
CODE_AFTER the cursor, output ONLY the code that belongs at the cursor. No \
explanations, no markdown fences, no repetition of surrounding code.";
        let user = format!(
            "CODE_BEFORE:\n{prefix}\n\nCODE_AFTER:\n{suffix}\n\nMIDDLE:"
        );

        let body = serde_json::json!({
            "model": self.config.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "max_tokens": self.config.max_tokens,
            "temperature": self.config.temperature,
        });

        let url = format!("{}/chat/completions", self.config.base_url);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.config.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("openrouter request failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("openrouter status {}", resp.status()));
        }

        let parsed: OpenRouterResponse = resp
            .json()
            .await
            .map_err(|e| format!("openrouter decode failed: {e}"))?;

        let text = parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap_or_default();
        Ok(clean_completion(&strip_code_fences(&text)))
    }
}

#[derive(Deserialize)]
struct OllamaGenerateResponse {
    #[serde(default)]
    response: String,
}

#[derive(Deserialize)]
struct OpenRouterResponse {
    #[serde(default)]
    choices: Vec<OpenRouterChoice>,
}

#[derive(Deserialize)]
struct OpenRouterChoice {
    message: OpenRouterMessage,
}

#[derive(Deserialize)]
struct OpenRouterMessage {
    #[serde(default)]
    content: String,
}

/// Trim trailing whitespace-only lines and stray sentinel fragments that some
/// models emit despite stop tokens.
fn clean_completion(raw: &str) -> String {
    let mut text = raw.to_string();
    for sentinel in [
        "<|endoftext|>",
        "<|fim_pad|>",
        "<|file_separator|>",
        "<EOT>",
        "<｜end▁of▁sentence｜>",
    ] {
        if let Some(idx) = text.find(sentinel) {
            text.truncate(idx);
        }
    }
    // Preserve internal newlines but drop a trailing run of blank lines.
    while text.ends_with('\n') {
        text.pop();
    }
    text
}

/// Remove markdown code fences a chat model may wrap its answer in.
fn strip_code_fences(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("```") {
        // Drop the optional language tag on the first line.
        let after_lang = rest.splitn(2, '\n').nth(1).unwrap_or("");
        return after_lang.trim_end_matches("```").trim_end().to_string();
    }
    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_qwen_family() {
        assert_eq!(FimFamily::detect("qwen2.5-coder:1.5b"), FimFamily::Qwen);
        assert_eq!(FimFamily::detect("unknown-model"), FimFamily::Qwen);
    }

    #[test]
    fn detects_deepseek_family() {
        assert_eq!(
            FimFamily::detect("deepseek-coder:6.7b"),
            FimFamily::DeepSeek
        );
        assert_eq!(
            FimFamily::detect("deepseek/deepseek-coder"),
            FimFamily::DeepSeek
        );
    }

    #[test]
    fn detects_starcoder_family() {
        assert_eq!(FimFamily::detect("starcoder2:3b"), FimFamily::StarCoder);
    }

    #[test]
    fn detects_codellama_family() {
        assert_eq!(FimFamily::detect("codellama:7b"), FimFamily::CodeLlama);
    }

    #[test]
    fn builds_psm_prompt_with_correct_tokens() {
        let p = FimFamily::Qwen.build_prompt("let x = ", ";");
        assert!(p.starts_with("<|fim_prefix|>let x = "));
        assert!(p.contains("<|fim_suffix|>;"));
        assert!(p.ends_with("<|fim_middle|>"));
    }

    #[test]
    fn deepseek_prompt_uses_its_sentinels() {
        let p = FimFamily::DeepSeek.build_prompt("a", "b");
        assert!(p.contains("<｜fim▁begin｜>a"));
        assert!(p.contains("<｜fim▁hole｜>b"));
        assert!(p.ends_with("<｜fim▁end｜>"));
    }

    #[test]
    fn clean_completion_strips_sentinels_and_trailing_blanks() {
        let out = clean_completion("foo()\n<|endoftext|>extra");
        assert_eq!(out, "foo()");
        let out2 = clean_completion("bar\n\n\n");
        assert_eq!(out2, "bar");
    }

    #[test]
    fn clean_completion_preserves_internal_newlines() {
        let out = clean_completion("line1\nline2\n");
        assert_eq!(out, "line1\nline2");
    }

    #[test]
    fn strip_code_fences_removes_wrapping() {
        let out = strip_code_fences("```rust\nlet x = 1;\n```");
        assert_eq!(out, "let x = 1;");
    }

    #[test]
    fn strip_code_fences_leaves_plain_text() {
        let out = strip_code_fences("let x = 1;");
        assert_eq!(out, "let x = 1;");
    }

    #[test]
    fn stop_tokens_are_family_specific() {
        assert!(FimFamily::Qwen.stop_tokens().iter().any(|s| s == "<|fim_pad|>"));
        assert!(FimFamily::CodeLlama
            .stop_tokens()
            .iter()
            .any(|s| s == "<EOT>"));
    }

    #[test]
    fn default_config_is_local_qwen() {
        let c = FimConfig::default();
        assert_eq!(c.backend, FimBackend::Ollama);
        assert!(c.model.contains("qwen"));
        assert_eq!(c.max_tokens, 128);
    }
}
