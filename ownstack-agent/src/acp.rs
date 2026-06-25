//! Agent Client Protocol (ACP) support.
//!
//! ACP is an open, JSON-RPC 2.0 standard (popularised by Zed) that lets any AI
//! agent integrate natively with any compatible editor. Implementing the
//! *agent* side here means OwnStack's agent can be driven by ACP-capable
//! editors, and conversely OwnStack can host external ACP agents.
//!
//! This module provides the protocol types and a dispatcher. The transport is
//! newline-delimited JSON-RPC over stdio (the ACP default). The dispatcher is
//! transport-agnostic and unit-tested; `run_stdio` wires it to real stdio.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Protocol version this implementation speaks.
pub const ACP_VERSION: &str = "0.1";

/// A JSON-RPC 2.0 request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// A JSON-RPC 2.0 response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
}

impl JsonRpcResponse {
    pub fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: Value, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

/// Capabilities advertised by the agent on `initialize`.
pub fn agent_capabilities() -> Value {
    json!({
        "protocolVersion": ACP_VERSION,
        "agentName": "ownstack-agent",
        "agentVersion": env!("CARGO_PKG_VERSION"),
        "capabilities": {
            "promptCapabilities": {
                "streaming": true,
                "tools": true,
                "image": true,
            },
            "editorCapabilities": {
                "openFile": true,
                "applyEdit": true,
                "readFile": true,
                "diagnostics": true,
                "terminal": true,
            },
            "modes": ["ask", "auto", "plan"],
        }
    })
}

/// Outcome of dispatching one ACP request.
#[derive(Debug, Clone, PartialEq)]
pub enum Dispatch {
    /// A response to send back (for requests carrying an id).
    Reply(JsonRpcResponse),
    /// A prompt the host should process (method `session/prompt`), carrying the
    /// request id (for the eventual reply) and the prompt text.
    Prompt { id: Value, session: String, text: String },
    /// A notification with no reply (e.g. `session/cancel`).
    Notify(String),
    /// Nothing to do (parse error already handled, or unknown notification).
    Ignore,
}

/// Standard JSON-RPC error codes.
pub mod codes {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
}

/// Dispatch a single raw JSON-RPC line, returning the action to take.
///
/// Handles the protocol handshake (`initialize`, `session/new`) directly and
/// surfaces `session/prompt` to the caller via [`Dispatch::Prompt`].
pub fn dispatch_line(line: &str) -> Dispatch {
    let req: JsonRpcRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(_) => return Dispatch::Ignore,
    };

    match req.method.as_str() {
        "initialize" => {
            let id = req.id.unwrap_or(Value::Null);
            Dispatch::Reply(JsonRpcResponse::ok(id, agent_capabilities()))
        }
        "session/new" => {
            let id = req.id.unwrap_or(Value::Null);
            let session_id = format!(
                "acp-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0)
            );
            Dispatch::Reply(JsonRpcResponse::ok(
                id,
                json!({ "sessionId": session_id }),
            ))
        }
        "session/prompt" => {
            let id = req.id.clone().unwrap_or(Value::Null);
            let params = req.params.unwrap_or(Value::Null);
            let session = params
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or("default")
                .to_string();
            let text = extract_prompt_text(&params);
            Dispatch::Prompt { id, session, text }
        }
        "session/cancel" => Dispatch::Notify("cancel".to_string()),
        other => {
            if let Some(id) = req.id {
                Dispatch::Reply(JsonRpcResponse::err(
                    id,
                    codes::METHOD_NOT_FOUND,
                    format!("unknown method: {other}"),
                ))
            } else {
                Dispatch::Ignore
            }
        }
    }
}

/// Extract the prompt text from `session/prompt` params. ACP carries a `prompt`
/// array of content blocks; we concatenate the text blocks. Falls back to a
/// plain `prompt` string for simpler clients.
fn extract_prompt_text(params: &Value) -> String {
    if let Some(s) = params.get("prompt").and_then(|v| v.as_str()) {
        return s.to_string();
    }
    if let Some(arr) = params.get("prompt").and_then(|v| v.as_array()) {
        let mut out = String::new();
        for block in arr {
            if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                out.push_str(t);
            }
        }
        return out;
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_returns_capabilities() {
        let line = r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
        match dispatch_line(line) {
            Dispatch::Reply(resp) => {
                assert_eq!(resp.id, json!(1));
                let result = resp.result.unwrap();
                assert_eq!(result["protocolVersion"], ACP_VERSION);
                assert_eq!(
                    result["capabilities"]["editorCapabilities"]["applyEdit"],
                    json!(true)
                );
            }
            other => panic!("expected reply, got {other:?}"),
        }
    }

    #[test]
    fn session_new_returns_session_id() {
        let line = r#"{"jsonrpc":"2.0","id":2,"method":"session/new"}"#;
        match dispatch_line(line) {
            Dispatch::Reply(resp) => {
                let result = resp.result.unwrap();
                assert!(result["sessionId"]
                    .as_str()
                    .unwrap()
                    .starts_with("acp-"));
            }
            other => panic!("expected reply, got {other:?}"),
        }
    }

    #[test]
    fn prompt_surfaces_text_from_string() {
        let line = r#"{"jsonrpc":"2.0","id":3,"method":"session/prompt",
            "params":{"sessionId":"s1","prompt":"hello world"}}"#;
        match dispatch_line(line) {
            Dispatch::Prompt { id, session, text } => {
                assert_eq!(id, json!(3));
                assert_eq!(session, "s1");
                assert_eq!(text, "hello world");
            }
            other => panic!("expected prompt, got {other:?}"),
        }
    }

    #[test]
    fn prompt_surfaces_text_from_content_blocks() {
        let line = r#"{"jsonrpc":"2.0","id":4,"method":"session/prompt",
            "params":{"prompt":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}}"#;
        match dispatch_line(line) {
            Dispatch::Prompt { text, session, .. } => {
                assert_eq!(text, "ab");
                assert_eq!(session, "default");
            }
            other => panic!("expected prompt, got {other:?}"),
        }
    }

    #[test]
    fn unknown_method_with_id_errors() {
        let line = r#"{"jsonrpc":"2.0","id":5,"method":"does/not/exist"}"#;
        match dispatch_line(line) {
            Dispatch::Reply(resp) => {
                let err = resp.error.unwrap();
                assert_eq!(err.code, codes::METHOD_NOT_FOUND);
            }
            other => panic!("expected error reply, got {other:?}"),
        }
    }

    #[test]
    fn unknown_notification_is_ignored() {
        let line = r#"{"jsonrpc":"2.0","method":"some/notification"}"#;
        assert_eq!(dispatch_line(line), Dispatch::Ignore);
    }

    #[test]
    fn cancel_is_a_notification() {
        let line = r#"{"jsonrpc":"2.0","method":"session/cancel"}"#;
        assert_eq!(dispatch_line(line), Dispatch::Notify("cancel".to_string()));
    }

    #[test]
    fn malformed_json_is_ignored() {
        assert_eq!(dispatch_line("not json at all"), Dispatch::Ignore);
    }

    #[test]
    fn response_constructors() {
        let ok = JsonRpcResponse::ok(json!(1), json!({"a": 1}));
        assert!(ok.error.is_none());
        assert_eq!(ok.jsonrpc, "2.0");
        let err = JsonRpcResponse::err(json!(2), -1, "boom");
        assert!(err.result.is_none());
        assert_eq!(err.error.unwrap().message, "boom");
    }
}
