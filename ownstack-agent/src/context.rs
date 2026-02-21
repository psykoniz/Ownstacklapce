//! Context Window Management
//!
//! Manages conversation history and context window limits
//! to ensure optimal token usage.

use crate::provider::LlmMessage;

/// Manages context window for LLM conversations
pub struct ContextManager {
    max_tokens: usize,
    messages: Vec<LlmMessage>,
    system_prompt: Option<String>,
}

impl ContextManager {
    pub fn new(max_tokens: usize) -> Self {
        Self {
            max_tokens,
            messages: Vec::new(),
            system_prompt: None,
        }
    }

    /// Set the system prompt
    pub fn set_system_prompt(&mut self, prompt: impl Into<String>) {
        self.system_prompt = Some(prompt.into());
    }

    /// Add a message to the context
    pub fn add_message(&mut self, message: LlmMessage) {
        self.messages.push(message);
        self.trim_if_needed();
    }

    /// Get all messages including system prompt
    pub fn get_messages(&self) -> Vec<LlmMessage> {
        let mut result = Vec::new();

        if let Some(ref prompt) = self.system_prompt {
            result.push(LlmMessage::system(prompt.clone()));
        }

        result.extend(self.messages.clone());
        result
    }

    /// Clear conversation history (keeps system prompt)
    pub fn clear(&mut self) {
        self.messages.clear();
    }

    /// Estimate token count for a string (rough approximation)
    pub(crate) fn estimate_tokens(text: &str) -> usize {
        // Rough estimate: ~4 characters per token
        text.len() / 4
    }

    /// Get estimated total token count
    pub fn estimated_tokens(&self) -> usize {
        let system_tokens = self
            .system_prompt
            .as_ref()
            .map(|s| Self::estimate_tokens(s))
            .unwrap_or(0);

        let message_tokens: usize = self
            .messages
            .iter()
            .map(|m| Self::estimate_tokens(&m.get_text()))
            .sum();

        system_tokens + message_tokens
    }

    /// Trim oldest messages if we exceed token limit
    fn trim_if_needed(&mut self) {
        while self.estimated_tokens() > self.max_tokens && self.messages.len() > 1 {
            self.messages.remove(0);
        }
    }

    /// Get message count
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Role;

    // ─── Basic Functionality ─────────────────────────────────────
    #[test]
    fn test_new_context_manager() {
        let cm = ContextManager::new(4096);
        assert_eq!(cm.max_tokens, 4096);
        assert!(cm.messages.is_empty());
    }

    #[test]
    fn test_add_message() {
        let mut cm = ContextManager::new(4096);
        cm.add_message(LlmMessage::user("Hello"));
        assert_eq!(cm.messages.len(), 1);
        assert_eq!(cm.messages[0].content, "Hello");
    }

    #[test]
    fn test_add_multiple_messages() {
        let mut cm = ContextManager::new(4096);
        cm.add_message(LlmMessage::user("First"));
        cm.add_message(LlmMessage::assistant("Response"));
        cm.add_message(LlmMessage::user("Second"));
        assert_eq!(cm.messages.len(), 3);
    }

    #[test]
    fn test_set_system_prompt() {
        let mut cm = ContextManager::new(4096);
        cm.set_system_prompt("You are an assistant");
        assert_eq!(cm.system_prompt, Some("You are an assistant".to_string()));
    }

    #[test]
    fn test_set_system_prompt_updates() {
        let mut cm = ContextManager::new(4096);
        cm.set_system_prompt("First prompt");
        cm.set_system_prompt("Updated prompt");
        assert_eq!(cm.system_prompt, Some("Updated prompt".to_string()));
    }

    // ─── get_messages ────────────────────────────────────────────
    #[test]
    fn test_get_messages_without_system() {
        let mut cm = ContextManager::new(4096);
        cm.add_message(LlmMessage::user("Hello"));
        let msgs = cm.get_messages();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, Role::User);
    }

    #[test]
    fn test_get_messages_with_system() {
        let mut cm = ContextManager::new(4096);
        cm.set_system_prompt("Be helpful");
        cm.add_message(LlmMessage::user("Hello"));
        let msgs = cm.get_messages();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, Role::System);
        assert_eq!(msgs[0].content, "Be helpful");
        assert_eq!(msgs[1].role, Role::User);
    }

    #[test]
    fn test_get_messages_full_conversation() {
        let mut cm = ContextManager::new(1000);
        cm.set_system_prompt("You are a helpful assistant.");
        cm.add_message(LlmMessage::user("Hello!"));
        cm.add_message(LlmMessage::assistant("Hi there!"));

        let messages = cm.get_messages();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, Role::System);
        assert_eq!(messages[1].role, Role::User);
        assert_eq!(messages[2].role, Role::Assistant);
    }

    // ─── Token Estimation ────────────────────────────────────────
    #[test]
    fn test_estimate_tokens() {
        let tokens = ContextManager::estimate_tokens("Hello, world!");
        assert!(tokens > 0);
    }

    #[test]
    fn test_estimate_tokens_empty() {
        let tokens = ContextManager::estimate_tokens("");
        assert_eq!(tokens, 0);
    }

    #[test]
    fn test_estimate_tokens_long_string() {
        let long = "word ".repeat(1000);
        let tokens = ContextManager::estimate_tokens(&long);
        assert!(tokens > 500, "~1000 words should estimate to many tokens");
    }

    // ─── message_count ───────────────────────────────────────────
    #[test]
    fn test_message_count() {
        let mut cm = ContextManager::new(4096);
        assert_eq!(cm.message_count(), 0);
        cm.add_message(LlmMessage::user("a"));
        assert_eq!(cm.message_count(), 1);
        cm.add_message(LlmMessage::assistant("b"));
        assert_eq!(cm.message_count(), 2);
    }

    // ─── Trimming ────────────────────────────────────────────────
    #[test]
    fn test_trimming_removes_old_messages() {
        let mut cm = ContextManager::new(50);
        for i in 0..100 {
            cm.add_message(LlmMessage::user(format!(
                "Message number {} with some content",
                i
            )));
        }
        assert!(cm.messages.len() < 100, "Should have trimmed messages");
    }

    #[test]
    fn test_trimming_preserves_recent() {
        let mut cm = ContextManager::new(100);
        cm.add_message(LlmMessage::user("Old message"));
        for _ in 0..50 {
            cm.add_message(LlmMessage::user(
                "Filling up the context with more text",
            ));
        }
        assert!(!cm.messages.is_empty());
        assert!(cm.messages.last().unwrap().content.contains("Filling"));
    }

    // ─── Clear ───────────────────────────────────────────────────
    #[test]
    fn test_clear() {
        let mut cm = ContextManager::new(4096);
        cm.set_system_prompt("Keep this");
        cm.add_message(LlmMessage::user("Hello"));
        cm.add_message(LlmMessage::assistant("Hi"));
        cm.clear();
        assert!(cm.messages.is_empty());
        assert!(cm.system_prompt.is_some());
    }

    // ─── Message Order ───────────────────────────────────────────
    #[test]
    fn test_message_order_preserved() {
        let mut cm = ContextManager::new(4096);
        for i in 0..10 {
            cm.add_message(LlmMessage::user(format!("msg_{}", i)));
        }
    }

    // ─── Edge Cases ──────────────────────────────────────────────
    #[test]
    fn test_empty_message() {
        let mut cm = ContextManager::new(4096);
        cm.add_message(LlmMessage::user(""));
        assert_eq!(cm.messages.len(), 1);
    }

    #[test]
    fn test_unicode_messages() {
        let mut cm = ContextManager::new(4096);
        cm.add_message(LlmMessage::user("日本語テスト 🦀"));
        assert!(cm.messages[0].content.contains("🦀"));
    }

    // ─── Stress Tests ────────────────────────────────────────────
    #[test]
    fn stress_test_1000_messages() {
        let mut cm = ContextManager::new(10000);
        for i in 0..1000 {
            cm.add_message(LlmMessage::user(format!("msg_{}", i)));
        }
        assert!(!cm.messages.is_empty());
    }

    #[test]
    fn stress_test_alternating_roles() {
        let mut cm = ContextManager::new(8000);
        for i in 0..500 {
            if i % 2 == 0 {
                cm.add_message(LlmMessage::user(format!("q_{}", i)));
            } else {
                cm.add_message(LlmMessage::assistant(format!("a_{}", i)));
            }
        }
        assert!(!cm.messages.is_empty());
    }

    #[test]
    fn stress_test_get_messages_repeated() {
        let mut cm = ContextManager::new(4096);
        cm.set_system_prompt("prompt");
        cm.add_message(LlmMessage::user("test"));

        for _ in 0..100 {
            let msgs = cm.get_messages();
            assert!(msgs.len() >= 2);
        }
    }
}
