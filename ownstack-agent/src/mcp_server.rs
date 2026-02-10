//! MCP Server — Expose OwnStack tools via MCP protocol
//!
//! Allows external clients to discover and call OwnStack tools
//! using the Model Context Protocol.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, error, info};

use crate::toolkits::{Toolkit, ToolDef};

// ─── MCP Server Protocol Types ─────────────────────────────────────

#[derive(Debug, Deserialize)]
struct IncomingRequest {
    jsonrpc: String,
    id: Option<u64>,
    method: String,
    params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct OutgoingResponse {
    jsonrpc: String,
    id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

impl OutgoingResponse {
    fn success(id: Option<u64>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Option<u64>, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(RpcError { code, message }),
        }
    }
}

// ─── MCP Server ────────────────────────────────────────────────────

/// MCP Server that exposes OwnStack toolkits to external clients
pub struct McpServer {
    toolkits: Vec<Arc<dyn Toolkit>>,
    server_name: String,
    server_version: String,
}

impl McpServer {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            toolkits: Vec::new(),
            server_name: name.into(),
            server_version: version.into(),
        }
    }

    /// Register a toolkit to expose via MCP
    pub fn register_toolkit(&mut self, toolkit: Arc<dyn Toolkit>) {
        self.toolkits.push(toolkit);
    }

    /// Get all tool definitions
    fn get_all_tools(&self) -> Vec<ToolDef> {
        self.toolkits.iter().flat_map(|tk| tk.tools()).collect()
    }

    /// Execute a tool by name
    async fn execute_tool(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        for toolkit in &self.toolkits {
            match toolkit.execute(tool_name, arguments.clone()).await {
                Ok(result) => {
                    let content = serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": result.output
                        }],
                        "isError": !result.success
                    });
                    return Ok(content);
                }
                Err(crate::toolkits::ToolkitError::ToolNotFound(_)) => continue,
                Err(e) => return Err(e.to_string()),
            }
        }
        Err(format!("Tool not found: {}", tool_name))
    }

    /// Handle a single incoming request
    async fn handle_request(&self, request: IncomingRequest) -> Option<OutgoingResponse> {
        debug!("MCP Server handling: {}", request.method);

        match request.method.as_str() {
            "initialize" => {
                let result = serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {
                            "listChanged": false
                        }
                    },
                    "serverInfo": {
                        "name": self.server_name,
                        "version": self.server_version
                    }
                });
                Some(OutgoingResponse::success(request.id, result))
            }

            "notifications/initialized" => {
                // Notification, no response
                None
            }

            "tools/list" => {
                let tools: Vec<serde_json::Value> = self
                    .get_all_tools()
                    .into_iter()
                    .map(|t| {
                        serde_json::json!({
                            "name": t.name,
                            "description": t.description,
                            "inputSchema": t.parameters
                        })
                    })
                    .collect();

                Some(OutgoingResponse::success(
                    request.id,
                    serde_json::json!({ "tools": tools }),
                ))
            }

            "tools/call" => {
                let params = request.params.unwrap_or_default();
                let tool_name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let arguments = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or(serde_json::json!({}));

                match self.execute_tool(tool_name, arguments).await {
                    Ok(result) => Some(OutgoingResponse::success(request.id, result)),
                    Err(e) => Some(OutgoingResponse::error(request.id, -32000, e)),
                }
            }

            _ => Some(OutgoingResponse::error(
                request.id,
                -32601,
                format!("Method not found: {}", request.method),
            )),
        }
    }

    /// Run the MCP server on stdio (blocking)
    pub async fn run_stdio(&self) -> Result<(), String> {
        info!("MCP Server '{}' starting on stdio", self.server_name);

        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut reader = BufReader::new(stdin);

        loop {
            let mut line = String::new();
            match reader.read_line(&mut line).await {
                Ok(0) => {
                    info!("MCP Server: stdin closed, shutting down");
                    return Ok(());
                }
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    match serde_json::from_str::<IncomingRequest>(trimmed) {
                        Ok(request) => {
                            if let Some(response) = self.handle_request(request).await {
                                let response_json = serde_json::to_string(&response)
                                    .map_err(|e| e.to_string())?;
                                stdout
                                    .write_all(response_json.as_bytes())
                                    .await
                                    .map_err(|e| e.to_string())?;
                                stdout
                                    .write_all(b"\n")
                                    .await
                                    .map_err(|e| e.to_string())?;
                                stdout.flush().await.map_err(|e| e.to_string())?;
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse request: {}", e);
                            let err_response = OutgoingResponse::error(
                                None,
                                -32700,
                                format!("Parse error: {}", e),
                            );
                            let json = serde_json::to_string(&err_response)
                                .map_err(|e| e.to_string())?;
                            stdout
                                .write_all(json.as_bytes())
                                .await
                                .map_err(|e| e.to_string())?;
                            stdout
                                .write_all(b"\n")
                                .await
                                .map_err(|e| e.to_string())?;
                            stdout.flush().await.map_err(|e| e.to_string())?;
                        }
                    }
                }
                Err(e) => {
                    error!("Read error: {}", e);
                    return Err(e.to_string());
                }
            }
        }

    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolkits::{ToolResult, ToolkitError};
    use async_trait::async_trait;

    struct MockToolkit;

    #[async_trait]
    impl Toolkit for MockToolkit {
        fn name(&self) -> &str { "mock" }
        fn tools(&self) -> Vec<ToolDef> {
            vec![ToolDef {
                name: "mock_tool".to_string(),
                description: "desc".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            }]
        }
        async fn execute(&self, name: &str, _args: serde_json::Value) -> Result<ToolResult, ToolkitError> {
            if name == "mock_tool" {
                Ok(ToolResult::success("mock output".to_string()))
            } else {
                Err(ToolkitError::ToolNotFound(name.to_string()))
            }
        }
    }

    #[tokio::test]
    async fn test_mcp_initialize() {
        let server = McpServer::new("test-server", "1.0.0");
        let req = IncomingRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(1),
            method: "initialize".to_string(),
            params: None,
        };
        let res = server.handle_request(req).await.unwrap();
        assert_eq!(res.id, Some(1));
        let result = res.result.unwrap();
        assert_eq!(result.get("serverInfo").unwrap().get("name").unwrap(), "test-server");
    }

    #[tokio::test]
    async fn test_mcp_tools_list() {
        let mut server = McpServer::new("test", "1.0");
        server.register_toolkit(Arc::new(MockToolkit));
        
        let req = IncomingRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(2),
            method: "tools/list".to_string(),
            params: None,
        };
        let res = server.handle_request(req).await.unwrap();
        let tools = res.result.unwrap().get("tools").unwrap().as_array().unwrap().clone();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].get("name").unwrap(), "mock_tool");
    }

    #[tokio::test]
    async fn test_mcp_tools_call() {
        let mut server = McpServer::new("test", "1.0");
        server.register_toolkit(Arc::new(MockToolkit));
        
        let req = IncomingRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(3),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "mock_tool",
                "arguments": {}
            })),
        };
        let res = server.handle_request(req).await.unwrap();
        let result = res.result.unwrap();
        let text = result.get("content").unwrap().as_array().unwrap()[0].get("text").unwrap();
        assert_eq!(text, "mock output");
    }

    #[tokio::test]
    async fn test_mcp_invalid_method() {
        let server = McpServer::new("test", "1.0");
        let req = IncomingRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(4),
            method: "unknown".to_string(),
            params: None,
        };
        let res = server.handle_request(req).await.unwrap();
        assert!(res.error.is_some());
        assert_eq!(res.error.unwrap().code, -32601);
    }
}
