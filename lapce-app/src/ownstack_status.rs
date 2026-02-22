use floem::prelude::{SignalGet, SignalUpdate};
use floem::reactive::{RwSignal, create_rw_signal};

use crate::ownstack_chat::AgentMode;
use crate::window_tab::CommonData;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnStackRunState {
    Running,
    Idle,
    Disconnected,
}

/// OwnStack status bar state.
#[derive(Clone)]
pub struct OwnStackStatusData {
    /// Current agent mode.
    pub mode: RwSignal<AgentMode>,
    /// Whether the agent is actively processing.
    pub is_active: RwSignal<bool>,
    /// Current status text.
    pub status_text: RwSignal<String>,
    /// Connection status to proxy/agent bridge.
    pub bridge_connected: RwSignal<bool>,
    /// Number of pending operations.
    pub pending_ops: RwSignal<u32>,
    #[allow(dead_code)]
    common: CommonData,
}

impl OwnStackStatusData {
    pub fn new(common: CommonData) -> Self {
        Self {
            mode: create_rw_signal(AgentMode::Ask),
            is_active: create_rw_signal(false),
            status_text: create_rw_signal("idle".to_string()),
            bridge_connected: create_rw_signal(false),
            pending_ops: create_rw_signal(0),
            common,
        }
    }

    /// Build the status bar label.
    pub fn display_label(&self) -> String {
        compose_display_label(
            &self.mode.get(),
            self.is_active.get(),
            self.bridge_connected.get(),
            &self.status_text.get(),
            self.pending_ops.get(),
        )
    }

    /// Build the state/detail section (without mode), used by status bar badges.
    pub fn detail_label(&self) -> String {
        compose_detail_label(
            self.is_active.get(),
            self.bridge_connected.get(),
            &self.status_text.get(),
            self.pending_ops.get(),
        )
    }

    pub fn mode_label(&self) -> &'static str {
        mode_label(&self.mode.get())
    }

    pub fn run_state(&self) -> OwnStackRunState {
        if self.is_active.get() {
            OwnStackRunState::Running
        } else if !self.bridge_connected.get() {
            OwnStackRunState::Disconnected
        } else {
            OwnStackRunState::Idle
        }
    }

    pub fn run_state_label(&self) -> &'static str {
        run_state_label(self.run_state())
    }

    /// Update the agent mode.
    pub fn set_mode(&self, mode: AgentMode) {
        tracing::info!("Status bar: mode -> {:?}", mode);
        self.mode.set(mode);
    }

    /// Set the active processing state.
    pub fn set_active(&self, active: bool) {
        self.is_active.set(active);
    }

    /// Set bridge connection status.
    pub fn set_bridge_connected(&self, connected: bool) {
        self.bridge_connected.set(connected);
    }

    /// Update status text.
    pub fn set_status(&self, text: impl Into<String>) {
        self.status_text.set(text.into());
    }

    /// Increment pending operations.
    pub fn push_op(&self) {
        self.pending_ops.update(|n| *n += 1);
    }

    /// Decrement pending operations.
    pub fn pop_op(&self) {
        self.pending_ops.update(|n| {
            if *n > 0 {
                *n -= 1;
            }
        });
    }
}

fn mode_label(mode: &AgentMode) -> &'static str {
    match mode {
        AgentMode::Ask => "Ask",
        AgentMode::Auto => "Auto",
        AgentMode::Plan => "Plan",
    }
}

fn run_state_label(state: OwnStackRunState) -> &'static str {
    match state {
        OwnStackRunState::Running => "running",
        OwnStackRunState::Idle => "idle",
        OwnStackRunState::Disconnected => "disconnected",
    }
}

pub(crate) fn compose_display_label(
    mode: &AgentMode,
    active: bool,
    connected: bool,
    detail: &str,
    pending_ops: u32,
) -> String {
    let mode_label = mode_label(mode);
    let detail_label = compose_detail_label(active, connected, detail, pending_ops);
    format!("OwnStack {mode_label} | {detail_label}")
}

pub(crate) fn compose_detail_label(
    active: bool,
    connected: bool,
    detail: &str,
    pending_ops: u32,
) -> String {
    let state = if active {
        OwnStackRunState::Running
    } else if !connected {
        OwnStackRunState::Disconnected
    } else {
        OwnStackRunState::Idle
    };
    let state_label = run_state_label(state);
    let detail = detail.trim();

    let mut label = if detail.is_empty() || detail == state_label {
        state_label.to_string()
    } else {
        format!("{state_label} ({detail})")
    };

    if pending_ops > 0 {
        label.push_str(&format!(" | ops:{pending_ops}"));
    }

    label
}

#[cfg(test)]
mod tests {
    use super::{compose_detail_label, compose_display_label};
    use crate::ownstack_chat::AgentMode;

    #[test]
    fn compose_label_idle_without_detail() {
        let label = compose_display_label(&AgentMode::Ask, false, true, "idle", 0);
        assert_eq!(label, "OwnStack Ask | idle");
    }

    #[test]
    fn compose_label_disconnected_with_detail() {
        let label = compose_display_label(
            &AgentMode::Auto,
            false,
            false,
            "handshake failed",
            0,
        );
        assert_eq!(label, "OwnStack Auto | disconnected (handshake failed)");
    }

    #[test]
    fn compose_label_running_with_pending_ops() {
        let label =
            compose_display_label(&AgentMode::Plan, true, true, "running", 2);
        assert_eq!(label, "OwnStack Plan | running | ops:2");
    }

    #[test]
    fn compose_detail_without_mode() {
        let label = compose_detail_label(false, true, "idle", 1);
        assert_eq!(label, "idle | ops:1");
    }
}
