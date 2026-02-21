use floem::prelude::{SignalGet, SignalUpdate};
use floem::reactive::{RwSignal, create_rw_signal};

use crate::ownstack_chat::AgentMode;
use crate::window_tab::CommonData;

/// OwnStack Status Bar — Shows agent mode and status
#[derive(Clone)]
pub struct OwnStackStatusData {
    /// Current agent mode
    pub mode: RwSignal<AgentMode>,
    /// Whether agent is actively processing
    pub is_active: RwSignal<bool>,
    /// Current status text
    pub status_text: RwSignal<String>,
    /// Connection status to Python bridge
    pub bridge_connected: RwSignal<bool>,
    /// Number of pending operations
    pub pending_ops: RwSignal<u32>,
    #[allow(dead_code)]
    common: CommonData,
}

impl OwnStackStatusData {
    pub fn new(common: CommonData) -> Self {
        Self {
            mode: create_rw_signal(AgentMode::Ask),
            is_active: create_rw_signal(false),
            status_text: create_rw_signal("OwnStack Ready".to_string()),
            bridge_connected: create_rw_signal(false),
            pending_ops: create_rw_signal(0),
            common,
        }
    }

    /// Get the display label for the status bar
    pub fn display_label(&self) -> String {
        let mode = self.mode.get_untracked();
        let active = self.is_active.get_untracked();
        let connected = self.bridge_connected.get_untracked();

        let mode_str = match mode {
            AgentMode::Ask => "🔵 Ask",
            AgentMode::Auto => "🟢 Auto",
            AgentMode::Plan => "🟡 Plan",
        };

        let status = if active {
            "⚡ Working..."
        } else if !connected {
            "⚠️ Bridge disconnected"
        } else {
            "✓ Ready"
        };

        format!("OwnStack {} | {}", mode_str, status)
    }

    /// Update the agent mode
    pub fn set_mode(&self, mode: AgentMode) {
        tracing::info!("Status bar: mode → {:?}", mode);
        self.mode.set(mode);
    }

    /// Set the active processing state
    pub fn set_active(&self, active: bool) {
        self.is_active.set(active);
    }

    /// Set bridge connection status
    pub fn set_bridge_connected(&self, connected: bool) {
        self.bridge_connected.set(connected);
    }

    /// Update status text
    pub fn set_status(&self, text: impl Into<String>) {
        self.status_text.set(text.into());
    }

    /// Increment pending operations
    pub fn push_op(&self) {
        self.pending_ops.update(|n| *n += 1);
    }

    /// Decrement pending operations
    pub fn pop_op(&self) {
        self.pending_ops.update(|n| {
            if *n > 0 {
                *n -= 1;
            }
        });
    }
}
