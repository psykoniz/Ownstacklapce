use floem::reactive::{RwSignal, create_rw_signal};
use floem::prelude::{SignalGet, SignalUpdate};
use lapce_rpc::ownstack::OwnStackRpc;
use serde::{Deserialize, Serialize};

use crate::window_tab::CommonData;

/// A single message in the AI chat
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    pub timestamp: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ChatRole {
    User,
    Assistant,
    System,
    Tool,
}

/// OwnStack Chat Panel — AI interaction interface
#[derive(Clone)]
pub struct OwnStackChatData {
    /// Current input text
    pub input: RwSignal<String>,
    /// Whether the chat panel is visible
    pub visible: RwSignal<bool>,
    /// Chat history
    pub messages: RwSignal<Vec<ChatMessage>>,
    /// Whether the agent is currently processing
    pub is_loading: RwSignal<bool>,
    /// Current agent mode
    pub agent_mode: RwSignal<AgentMode>,
    #[allow(dead_code)]
    common: CommonData,
}

/// Agent execution mode
#[derive(Clone, Debug, PartialEq)]
pub enum AgentMode {
    Ask,
    Auto,
    Plan,
}

impl std::fmt::Display for AgentMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentMode::Ask => write!(f, "Ask"),
            AgentMode::Auto => write!(f, "Auto"),
            AgentMode::Plan => write!(f, "Plan"),
        }
    }
}

impl OwnStackChatData {
    pub fn new(common: CommonData) -> Self {
        Self {
            input: create_rw_signal(String::new()),
            visible: create_rw_signal(false),
            messages: create_rw_signal(Vec::new()),
            is_loading: create_rw_signal(false),
            agent_mode: create_rw_signal(AgentMode::Ask),
            common,
        }
    }

    /// Toggle chat panel visibility
    pub fn toggle(&self) {
        let current = self.visible.get_untracked();
        self.visible.set(!current);
    }

    /// Show the chat panel
    pub fn show(&self) {
        self.visible.set(true);
    }

    /// Hide the chat panel
    pub fn hide(&self) {
        self.visible.set(false);
    }

    /// Send a message from the user
    pub fn send_message(&self) {
        let content = self.input.get_untracked();
        if content.trim().is_empty() {
            return;
        }

        // Add user message to history
        let user_msg = ChatMessage {
            role: ChatRole::User,
            content: content.clone(),
            timestamp: chrono_now(),
        };

        self.messages.update(|msgs| {
            msgs.push(user_msg);
        });

        // Clear input
        self.input.set(String::new());

        // Set loading state
        self.is_loading.set(true);

        // Send via RPC
        let message = OwnStackRpc::AiPrompt { prompt: content };
        tracing::info!("OwnStack Chat: {:?}", message);

        // TODO: Wire to proxy RPC for bridge forwarding
    }

    /// Receive a response from the AI
    pub fn receive_response(&self, content: String) {
        let assistant_msg = ChatMessage {
            role: ChatRole::Assistant,
            content,
            timestamp: chrono_now(),
        };

        self.messages.update(|msgs| {
            msgs.push(assistant_msg);
        });

        self.is_loading.set(false);
    }

    /// Add a system message
    pub fn add_system_message(&self, content: String) {
        let sys_msg = ChatMessage {
            role: ChatRole::System,
            content,
            timestamp: chrono_now(),
        };

        self.messages.update(|msgs| {
            msgs.push(sys_msg);
        });
    }

    /// Clear chat history
    pub fn clear_history(&self) {
        self.messages.set(Vec::new());
    }

    /// Cycle through agent modes: Ask → Auto → Plan → Ask
    pub fn cycle_mode(&self) {
        let current = self.agent_mode.get_untracked();
        let next = match current {
            AgentMode::Ask => AgentMode::Auto,
            AgentMode::Auto => AgentMode::Plan,
            AgentMode::Plan => AgentMode::Ask,
        };
        tracing::info!("Agent mode changed: {:?} → {:?}", current, next);
        self.agent_mode.set(next);
    }

    /// Get message count
    pub fn message_count(&self) -> usize {
        self.messages.get_untracked().len()
    }
}

/// Simple timestamp helper (avoids chrono dependency in lapce-app)
fn chrono_now() -> String {
    // Use a simple counter-based approach since we can't easily add chrono
    // In production, this would use proper timestamps
    format!("{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs())
}
