//! MCP (Model Context Protocol) Client & Toolkit
//!
//! Full MCP client implementation for connecting to external MCP servers.
//! Supports tool discovery and execution via JSON-RPC over stdio.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, info};

use super::{ToolDef, ToolResult, Toolkit, ToolkitError};

// ─── MCP Protocol Types ─────────────────────────────────────────────

/// JSON-RPC request
#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

/// JSON-RPC response
#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    _jsonrpc: String,
    #[allow(dead_code)]
    _id: Option<u64>,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

/// MCP Tool definition from server
#[derive(Debug, Clone, Deserialize)]
pub struct McpToolInfo {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "inputSchema")]
    pub input_schema: Option<serde_json::Value>,
}

/// MCP tool call result
#[derive(Debug, Deserialize)]
struct McpToolResult {
    content: Vec<McpContent>,
    #[serde(default)]
    is_error: bool,
}

#[derive(Debug, Deserialize)]
struct McpContent {
    #[serde(rename = "type")]
    _content_type: String,
    text: Option<String>,
}

/// Server capabilities from initialize
#[derive(Debug, Deserialize, Default)]
struct ServerCapabilities {
    #[serde(default)]
    _tools: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct InitializeResult {
    #[allow(dead_code)]
    _protocol_version: Option<String>,
    #[allow(dead_code)]
    _capabilities: Option<ServerCapabilities>,
    #[allow(dead_code)]
    _server_info: Option<ServerInfo>,
}

#[derive(Debug, Deserialize)]
struct ServerInfo {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    version: Option<String>,
}

// ─── MCP Client ─────────────────────────────────────────────────────

/// Configuration for an MCP server connection
#[derive(Debug, Clone)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
}

/// Connection to a single MCP server
struct McpConnection {
    child: Child,
    stdin: tokio::process::ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
    next_id: u64,
    tools: Vec<McpToolInfo>,
}

impl McpConnection {
    async fn send_request(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, ToolkitError> {
        self.next_id += 1;
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: self.next_id,
            method: method.to_string(),
            params,
        };

        let request_json = serde_json::to_string(&request).map_err(|e| {
            ToolkitError::ExecutionFailed(format!("Serialize error: {}", e))
        })?;

        debug!("MCP -> {}: {}", method, request_json);

        self.stdin
            .write_all(request_json.as_bytes())
            .await
            .map_err(|e| {
                ToolkitError::ExecutionFailed(format!("Write error: {}", e))
            })?;
        self.stdin.write_all(b"\n").await.map_err(|e| {
            ToolkitError::ExecutionFailed(format!("Write error: {}", e))
        })?;
        self.stdin.flush().await.map_err(|e| {
            ToolkitError::ExecutionFailed(format!("Flush error: {}", e))
        })?;

        // Read response
        let mut line = String::new();
        self.stdout.read_line(&mut line).await.map_err(|e| {
            ToolkitError::ExecutionFailed(format!("Read error: {}", e))
        })?;

        debug!("MCP <- {}", line.trim());

        let response: JsonRpcResponse =
            serde_json::from_str(line.trim()).map_err(|e| {
                ToolkitError::ExecutionFailed(format!("Parse error: {}", e))
            })?;

        if let Some(err) = response.error {
            return Err(ToolkitError::ExecutionFailed(format!(
                "MCP error {}: {}",
                err.code, err.message
            )));
        }

        response.result.ok_or_else(|| {
            ToolkitError::ExecutionFailed("No result in response".to_string())
        })
    }

    async fn shutdown(&mut self) {
        let _ = self.child.kill().await;
    }
}

/// MCP Client managing multiple server connections
pub struct McpClient {
    connections: HashMap<String, Mutex<McpConnection>>,
}

impl McpClient {
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
        }
    }

    /// Connect to an MCP server
    pub async fn connect(
        &mut self,
        config: McpServerConfig,
    ) -> Result<(), ToolkitError> {
        info!(
            "Connecting to MCP server: {} ({})",
            config.name, config.command
        );

        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        for (key, value) in &config.env {
            cmd.env(key, value);
        }

        let mut child = cmd.spawn().map_err(|e| {
            ToolkitError::ExecutionFailed(format!(
                "Failed to spawn MCP server: {}",
                e
            ))
        })?;

        let stdin = child.stdin.take().ok_or_else(|| {
            ToolkitError::ExecutionFailed("Failed to capture stdin".to_string())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            ToolkitError::ExecutionFailed("Failed to capture stdout".to_string())
        })?;

        let mut conn = McpConnection {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 0,
            tools: Vec::new(),
        };

        // Initialize handshake
        let init_params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "ownstack-ide",
                "version": "0.1.0"
            }
        });

        let init_result = conn.send_request("initialize", Some(init_params)).await?;
        let _init: InitializeResult =
            serde_json::from_value(init_result).map_err(|e| {
                ToolkitError::ExecutionFailed(format!("Init parse error: {}", e))
            })?;

        // Send initialized notification (no response expected for notifications)
        let notif = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        let notif_json = serde_json::to_string(&notif)
            .map_err(|e| ToolkitError::ExecutionFailed(e.to_string()))?;
        conn.stdin
            .write_all(notif_json.as_bytes())
            .await
            .map_err(|e| ToolkitError::ExecutionFailed(e.to_string()))?;
        conn.stdin
            .write_all(b"\n")
            .await
            .map_err(|e| ToolkitError::ExecutionFailed(e.to_string()))?;
        conn.stdin
            .flush()
            .await
            .map_err(|e| ToolkitError::ExecutionFailed(e.to_string()))?;

        // Discover tools
        let tools_result = conn.send_request("tools/list", None).await?;
        if let Some(tools_array) = tools_result.get("tools") {
            let tools: Vec<McpToolInfo> =
                serde_json::from_value(tools_array.clone()).unwrap_or_default();
            info!("Discovered {} tools from {}", tools.len(), config.name);
            conn.tools = tools;
        }

        let server_name = config.name.clone();
        self.connections.insert(server_name, Mutex::new(conn));

        Ok(())
    }

    /// Call a tool on a specific server
    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<ToolResult, ToolkitError> {
        let conn_mutex = self.connections.get(server_name).ok_or_else(|| {
            ToolkitError::ToolNotFound(format!(
                "MCP server not connected: {}",
                server_name
            ))
        })?;

        let mut conn = conn_mutex.lock().await;

        let params = serde_json::json!({
            "name": tool_name,
            "arguments": arguments,
        });

        let result = conn.send_request("tools/call", Some(params)).await?;

        let tool_result: McpToolResult =
            serde_json::from_value(result).map_err(|e| {
                ToolkitError::ExecutionFailed(format!("Parse result error: {}", e))
            })?;

        let output: String = tool_result
            .content
            .iter()
            .filter_map(|c| c.text.as_ref())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");

        if tool_result.is_error {
            Ok(ToolResult::failure(output, None))
        } else {
            Ok(ToolResult::success(output))
        }
    }

    /// Disconnect all servers
    pub async fn disconnect_all(&mut self) {
        for (name, conn_mutex) in self.connections.drain() {
            info!("Disconnecting MCP server: {}", name);
            let mut conn = conn_mutex.into_inner();
            conn.shutdown().await;
        }
    }
}

/// MCP toolkit that exposes MCP server tools to the agent
pub struct McpToolkit {
    client: McpClient,
    /// Cached tool definitions with server mapping
    tool_map: HashMap<String, (String, McpToolInfo)>, // tool_name -> (server_name, info)
}

impl McpToolkit {
    pub fn new() -> Self {
        Self {
            client: McpClient::new(),
            tool_map: HashMap::new(),
        }
    }

    /// Connect to an MCP server and register its tools
    pub async fn add_server(
        &mut self,
        config: McpServerConfig,
    ) -> Result<(), ToolkitError> {
        let server_name = config.name.clone();
        self.client.connect(config).await?;

        // Cache tools from this server
        if let Some(conn_mutex) = self.client.connections.get(&server_name) {
            let conn = conn_mutex.lock().await;
            for tool in &conn.tools {
                let prefixed_name = format!("mcp_{}_{}", server_name, tool.name);
                self.tool_map
                    .insert(prefixed_name, (server_name.clone(), tool.clone()));
            }
        }

        Ok(())
    }
}

impl Default for McpToolkit {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Toolkit for McpToolkit {
    fn name(&self) -> &str {
        "mcp"
    }

    fn tools(&self) -> Vec<ToolDef> {
        self.tool_map
            .iter()
            .map(|(name, (_server, info))| ToolDef {
                name: name.clone(),
                description: info.description.clone().unwrap_or_default(),
                parameters: info.input_schema.clone().unwrap_or(serde_json::json!({
                    "type": "object",
                    "properties": {}
                })),
            })
            .collect()
    }

    async fn execute(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<ToolResult, ToolkitError> {
        let (server_name, info) = self.tool_map.get(tool_name).ok_or_else(|| {
            ToolkitError::ToolNotFound(format!("MCP tool not found: {}", tool_name))
        })?;

        self.client.call_tool(server_name, &info.name, args).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_rpc_serialization() {
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "test".to_string(),
            params: Some(serde_json::json!({"a": 1})),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"method\":\"test\""));
        assert!(json.contains("\"id\":1"));
    }

    #[test]
    fn test_mcp_toolkit_name() {
        let toolkit = McpToolkit::new();
        assert_eq!(toolkit.name(), "mcp");
    }
}
