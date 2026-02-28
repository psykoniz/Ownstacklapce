use serde::{Deserialize, Serialize};

// ─── Agent Mode ──────────────────────────────────────────────────────────────

/// The operating mode of the agent (serialisable across RPC boundary)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentModeState {
    /// Automatically approve all policy decisions
    Auto,
    /// Ask the user before executing sensitive operations
    Ask,
    /// Plan mode: produce a full plan before executing
    Plan,
}

// ─── Agent Run State ─────────────────────────────────────────────────────────

/// Lifecycle state of the running agent
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunState {
    /// No agent process / bridge not connected
    Disconnected,
    /// Connected and waiting for prompts
    Idle,
    /// Actively running a mission or tool calls
    Running,
    /// Paused, waiting for user approval (Ask mode)
    AwaitingApproval,
    /// Stopped cleanly (kill-switch or budget exceeded)
    Stopped,
    /// Terminated due to an unrecoverable error
    Error,
}

// ─── Alert Severity ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertSeverity {
    Info,
    Warning,
    Error,
}

// ─── UiStateDelta sub-structs ─────────────────────────────────────────────────

/// Budget counters sent as part of a UI delta
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetState {
    pub tokens: u64,
    pub max_tokens: u64,
    pub steps: u64,
    pub max_steps: u64,
    pub calls: u64,
    pub max_calls: u64,
}

/// Context-window usage sent as part of a UI delta
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextState {
    pub current: u64,
    pub max: u64,
}

/// Mission snapshot sent as part of a UI delta
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionDelta {
    pub goal: String,
    pub steps: Vec<(String, String)>, // (description, status)
}

/// A pending approval request sent as part of a UI delta
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingApproval {
    pub command: String,
    pub reason: String,
}

/// Tool-call event summary sent as part of a UI delta
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolEvent {
    pub tool_name: String,
    pub status: String,
    pub summary: Option<String>,
}

/// An alert message sent as part of a UI delta
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertDelta {
    pub severity: AlertSeverity,
    pub message: String,
}

/// Incremental UI-state update emitted by the agent/proxy
///
/// All fields are optional – only the changed sub-states need to be set.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UiStateDelta {
    pub mode: Option<AgentModeState>,
    pub run_state: Option<AgentRunState>,
    pub budget: Option<BudgetState>,
    pub context: Option<ContextState>,
    pub mission: Option<MissionDelta>,
    pub pending_approval: Option<PendingApproval>,
    pub tool_event: Option<ToolEvent>,
    pub alert: Option<AlertDelta>,
}

// ─── OwnStackRpc ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum OwnStackRpc {
    /// Request the agent to start a new task
    AiPrompt { prompt: String },
    /// Stop the current agent operation and kill any running subprocesses.
    ///
    /// This is handled by the proxy (which owns the agent process handle).
    KillSwitch,
    /// Request execution of a tool (from agent to IDE)
    ToolExec { command: String, tool_name: String },
    /// Notification of an AI stream chunk
    AiStreamChunk {
        content_delta: Option<String>,
        tool_call_delta: Option<serde_json::Value>,
        finish_reason: Option<String>,
    },
    /// UI prompt for policy decision (Ask mode).
    /// `correlation_id` must be echoed in the matching `PolicyResponse`.
    /// `timeout_secs` tells the UI how long to wait before auto-deny.
    PolicyPrompt {
        command: String,
        reason: String,
        /// Optional CWD for display purposes.
        cwd: Option<String>,
        /// Unique id for this prompt — matched in PolicyResponse.
        correlation_id: String,
        /// Seconds before the UI auto-denies (0 means no forced timeout).
        timeout_secs: u32,
    },
    /// User (or auto-timeout) response to a policy prompt.
    PolicyResponse {
        approved: bool,
        /// Must match the `correlation_id` from the corresponding PolicyPrompt.
        correlation_id: String,
    },
    /// Audit event notification
    AuditEvent { json_entry: String },
    /// Tool execution result
    ToolResultMsg { json_result: String },
    /// Update on mission status (Phase 2 Mission System)
    MissionUpdate {
        goal: String,
        steps: Vec<(String, String)>, // (description, status)
    },
    /// User decision on an AI suggestion (Accept/Reject/Discuss)
    SuggestionDecision {
        decision: String, // "accept", "reject", "discuss"
        message_id: String,
    },
    /// Export UI metadata for agent vision
    UiSnapshot { metadata: String },
    /// Trigger a physical screenshot
    CaptureScreenshot,
    /// Request a UI snapshot from the agent side
    UiSnapshotRequest,
    /// Change the agent operating mode (Ask / Auto / Plan)
    SetAgentMode { mode: AgentModeState },
    /// Live budget counter update
    BudgetUpdate {
        tokens: u64,
        max_tokens: u64,
        steps: u64,
        max_steps: u64,
        calls: u64,
        max_calls: u64,
    },
    /// Context-window usage update
    ContextUpdate { current: u64, max: u64 },
    /// Incremental UI-state delta (preferred over individual update messages)
    UiStateDelta { delta: UiStateDelta },
}
