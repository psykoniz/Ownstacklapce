use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum OwnStackRpc {
    /// Request the agent to start a new task
    AiPrompt { 
        prompt: String 
    },
    /// Request execution of a tool (from agent to IDE)
    ToolExec { 
        command: String, 
        tool_name: String 
    },
    /// Notification of an AI stream chunk
    AiStreamChunk { 
        chunk: String 
    },
    /// UI prompt for policy decision (Ask mode)
    PolicyPrompt { 
        command: String, 
        reason: String 
    },
    /// User response to a policy prompt
    PolicyResponse { 
        approved: bool 
    },
    /// Audit event notification
    AuditEvent { 
        json_entry: String 
    },
    /// Tool execution result
    ToolResultMsg { 
        json_result: String 
    },
}
