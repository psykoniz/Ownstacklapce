use serde::{Deserialize, Serialize};

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
    /// UI prompt for policy decision (Ask mode)
    PolicyPrompt { command: String, reason: String },
    /// User response to a policy prompt
    PolicyResponse { approved: bool },
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
}
