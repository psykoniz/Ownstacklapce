use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentModeState {
    Ask,
    Auto,
    Plan,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunState {
    Disconnected,
    Idle,
    Running,
    AwaitingApproval,
    Stopped,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AlertSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BudgetSnapshot {
    pub tokens: u64,
    pub max_tokens: u64,
    pub steps: u64,
    pub max_steps: u64,
    pub calls: u64,
    pub max_calls: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextSnapshot {
    pub current: u64,
    pub max: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MissionSnapshot {
    pub goal: String,
    pub steps: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingApprovalSnapshot {
    pub command: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolEventSnapshot {
    pub tool_name: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AlertSnapshot {
    pub severity: AlertSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct UiStateDelta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<AgentModeState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_state: Option<AgentRunState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<BudgetSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mission: Option<MissionSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_approval: Option<PendingApprovalSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_event: Option<ToolEventSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alert: Option<AlertSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum OwnStackRpc {
    AiPrompt {
        prompt: String,
    },
    SetAgentMode {
        mode: AgentModeState,
    },
    KillSwitch,
    ToolExec {
        command: String,
        tool_name: String,
    },
    AiStreamChunk {
        content_delta: Option<String>,
        tool_call_delta: Option<serde_json::Value>,
        finish_reason: Option<String>,
    },
    PolicyPrompt {
        command: String,
        reason: String,
        cwd: Option<String>,
        correlation_id: String,
        timeout_secs: u32,
    },
    PolicyResponse {
        approved: bool,
        correlation_id: String,
    },
    AuditEvent {
        json_entry: String,
    },
    ToolResultMsg {
        json_result: String,
    },
    MissionUpdate {
        goal: String,
        steps: Vec<(String, String)>,
    },
    BudgetUpdate {
        tokens: u64,
        max_tokens: u64,
        steps: u64,
        max_steps: u64,
        calls: u64,
        max_calls: u64,
    },
    ContextUpdate {
        current: u64,
        max: u64,
    },
    SuggestionDecision {
        decision: String,
        message_id: String,
    },
    UiSnapshot {
        metadata: String,
    },
    CaptureScreenshot,
    UiSnapshotRequest,
    UiStateDelta {
        delta: UiStateDelta,
    },
}

#[cfg(test)]
mod tests {
    use super::{
        AgentModeState, AgentRunState, BudgetSnapshot, ContextSnapshot, OwnStackRpc,
        UiStateDelta,
    };

    #[test]
    fn legacy_variant_deserializes() {
        let json = r#"{"method":"ai_prompt","params":{"prompt":"hello"}}"#;
        let msg: OwnStackRpc =
            serde_json::from_str(json).expect("legacy ai_prompt should parse");
        match msg {
            OwnStackRpc::AiPrompt { prompt } => assert_eq!(prompt, "hello"),
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn ui_state_delta_roundtrip() {
        let msg = OwnStackRpc::UiStateDelta {
            delta: UiStateDelta {
                mode: Some(AgentModeState::Ask),
                run_state: Some(AgentRunState::Running),
                budget: Some(BudgetSnapshot {
                    tokens: 10,
                    max_tokens: 100,
                    steps: 1,
                    max_steps: 50,
                    calls: 2,
                    max_calls: 100,
                }),
                context: Some(ContextSnapshot {
                    current: 123,
                    max: 1000,
                }),
                mission: None,
                pending_approval: None,
                tool_event: None,
                alert: None,
            },
        };

        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: OwnStackRpc = serde_json::from_str(&json).expect("deserialize");
        match decoded {
            OwnStackRpc::UiStateDelta { delta } => {
                assert_eq!(delta.mode, Some(AgentModeState::Ask));
                assert_eq!(delta.run_state, Some(AgentRunState::Running));
                assert!(delta.budget.is_some());
                assert!(delta.context.is_some());
            }
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn set_agent_mode_roundtrip() {
        let msg = OwnStackRpc::SetAgentMode {
            mode: AgentModeState::Plan,
        };
        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: OwnStackRpc = serde_json::from_str(&json).expect("deserialize");
        match decoded {
            OwnStackRpc::SetAgentMode { mode } => {
                assert_eq!(mode, AgentModeState::Plan);
            }
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn policy_prompt_roundtrip_with_correlation() {
        let msg = OwnStackRpc::PolicyPrompt {
            command: "git push origin main".to_string(),
            reason: "publishing code".to_string(),
            cwd: Some("/workspace/project".to_string()),
            correlation_id: "policy-123".to_string(),
            timeout_secs: 15,
        };
        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: OwnStackRpc = serde_json::from_str(&json).expect("deserialize");
        match decoded {
            OwnStackRpc::PolicyPrompt {
                correlation_id,
                timeout_secs,
                ..
            } => {
                assert_eq!(correlation_id, "policy-123");
                assert_eq!(timeout_secs, 15);
            }
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[test]
    fn legacy_stream_chunk_still_deserializes() {
        let json = r#"{
            "method":"ai_stream_chunk",
            "params":{"content_delta":"hi","tool_call_delta":null,"finish_reason":null}
        }"#;
        let msg: OwnStackRpc =
            serde_json::from_str(json).expect("legacy stream chunk should parse");
        match msg {
            OwnStackRpc::AiStreamChunk { content_delta, .. } => {
                assert_eq!(content_delta.as_deref(), Some("hi"));
            }
            other => panic!("unexpected variant: {:?}", other),
        }
    }
}
