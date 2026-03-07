//! Mission Models — Data types for the mission system.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;

// ─── Mission Status ──────────────────────────────────────────────

/// Lifecycle status of a mission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionStatus {
    Created,
    Planning,
    Running,
    Verifying,
    NeedsReview,
    Completed,
    Failed,
    Cancelled,
}

impl std::fmt::Display for MissionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Created => write!(f, "created"),
            Self::Planning => write!(f, "planning"),
            Self::Running => write!(f, "running"),
            Self::Verifying => write!(f, "verifying"),
            Self::NeedsReview => write!(f, "needs_review"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

// ─── Mission Mode (Security Level) ──────────────────────────────

/// Security mode controlling what the agent may do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionMode {
    /// Pure reading: grep, cat, file listing only.
    StaticRead,
    /// Non-destructive tools: LSP, linters, search.
    SafeTooling,
    /// Full execution in ephemeral sandbox.
    DynamicExec,
    /// Planning only, no execution.
    Hypothetical,
}

impl Default for MissionMode {
    fn default() -> Self {
        Self::SafeTooling
    }
}

// ─── Execution Strategy ─────────────────────────────────────────

/// How the mission interacts with the workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStrategy {
    /// Work on a temporary git branch.
    EphemeralBranch,
    /// Log patches without applying.
    PatchLog,
    /// Simulate without side effects.
    DryRun,
}

impl Default for ExecutionStrategy {
    fn default() -> Self {
        Self::DryRun
    }
}

// ─── Permission ─────────────────────────────────────────────────

/// Granular permissions for a mission.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    FsRead,
    FsWrite,
    Exec,
    Network,
    Git,
    Lsp,
}

// ─── Mission Spec (compiled from prompt) ─────────────────────────

/// Technical contract compiled from a natural language prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionSpec {
    pub mode: MissionMode,
    #[serde(default)]
    pub strategy: ExecutionStrategy,
    pub objectives: Vec<String>,
    #[serde(default)]
    pub scope: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    #[serde(default)]
    pub oracles: Vec<String>,
    #[serde(default)]
    pub stop_conditions: Vec<String>,
    #[serde(default)]
    pub output_format: Vec<String>,
    #[serde(default)]
    pub budget_tokens: Option<u64>,
    #[serde(default)]
    pub preflight_checks: HashMap<String, bool>,
}

impl Default for MissionSpec {
    fn default() -> Self {
        Self {
            mode: MissionMode::default(),
            strategy: ExecutionStrategy::default(),
            objectives: Vec::new(),
            scope: vec![".".to_string()],
            permissions: vec![Permission::FsRead],
            oracles: Vec::new(),
            stop_conditions: Vec::new(),
            output_format: Vec::new(),
            budget_tokens: None,
            preflight_checks: HashMap::new(),
        }
    }
}

// ─── Mission Event ──────────────────────────────────────────────

/// A log event within a mission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionEvent {
    pub timestamp: f64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

impl MissionEvent {
    pub fn new(message: impl Into<String>) -> Self {
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        Self {
            timestamp: ts,
            message: message.into(),
            metadata: None,
        }
    }

    pub fn with_metadata(mut self, key: &str, value: serde_json::Value) -> Self {
        self.metadata
            .get_or_insert_with(HashMap::new)
            .insert(key.to_string(), value);
        self
    }
}

// ─── Checkpoint ─────────────────────────────────────────────────

/// A snapshot of the mission state that can be restored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub step_index: usize,
    pub created_at: f64,
    pub description: String,
    /// Optional git commit hash for code-level rollback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_hash: Option<String>,
}

// ─── Mission Record (persistent) ─────────────────────────────────

/// Full persisted mission record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionRecord {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: MissionStatus,
    #[serde(default)]
    pub spec: Option<MissionSpec>,
    #[serde(default)]
    pub events: Vec<MissionEvent>,
    #[serde(default)]
    pub checkpoints: Vec<Checkpoint>,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    pub created_at: f64,
    pub updated_at: f64,
}

impl MissionRecord {
    pub fn new(id: &str, title: &str, description: &str) -> Self {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        Self {
            id: id.to_string(),
            title: title.to_string(),
            description: description.to_string(),
            status: MissionStatus::Created,
            spec: None,
            events: Vec::new(),
            checkpoints: Vec::new(),
            metadata: HashMap::new(),
            created_at: now,
            updated_at: now,
        }
    }

    fn touch(&mut self) {
        self.updated_at = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
    }

    pub fn add_event(&mut self, event: MissionEvent) {
        self.events.push(event);
        self.touch();
    }

    pub fn set_status(&mut self, status: MissionStatus) {
        self.status = status;
        self.touch();
    }
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mission_record_lifecycle() {
        let mut m =
            MissionRecord::new("m-001", "Fix auth bug", "Fix the login flow");
        assert_eq!(m.status, MissionStatus::Created);

        m.set_status(MissionStatus::Planning);
        assert_eq!(m.status, MissionStatus::Planning);

        m.add_event(MissionEvent::new("Started planning"));
        assert_eq!(m.events.len(), 1);
        assert!(m.updated_at >= m.created_at);
    }

    #[test]
    fn test_mission_spec_serialization() {
        let spec = MissionSpec {
            mode: MissionMode::DynamicExec,
            strategy: ExecutionStrategy::EphemeralBranch,
            objectives: vec!["Fix the bug".to_string()],
            oracles: vec!["cargo test".to_string()],
            ..Default::default()
        };

        let json = serde_json::to_string(&spec).unwrap();
        let parsed: MissionSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.mode, MissionMode::DynamicExec);
        assert_eq!(parsed.objectives, vec!["Fix the bug"]);
    }

    #[test]
    fn test_event_with_metadata() {
        let event = MissionEvent::new("Step completed")
            .with_metadata("step_id", serde_json::json!(1))
            .with_metadata("tool_name", serde_json::json!("search"));

        assert_eq!(event.metadata.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_mission_status_display() {
        assert_eq!(MissionStatus::NeedsReview.to_string(), "needs_review");
        assert_eq!(MissionStatus::Running.to_string(), "running");
    }
}
