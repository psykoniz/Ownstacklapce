//! MCP (Model Context Protocol) Client & Toolkit
//!
//! JSON-RPC over stdio client with:
//! - initialize handshake
//! - tool discovery + tool calls
//! - resource/prompt utility endpoints
//! - response id matching and notification skipping

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, info};

use super::{ToolDef, ToolResult, Toolkit, ToolkitError};

const MCP_RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
const VIRTUAL_LIST_RESOURCES: &str = "__list_resources";
const VIRTUAL_READ_RESOURCE: &str = "__read_resource";
const VIRTUAL_LIST_PROMPTS: &str = "__list_prompts";
const VIRTUAL_GET_PROMPT: &str = "__get_prompt";

#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcNotification {
    jsonrpc: String,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<serde_json::Value>,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct McpToolInfo {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "inputSchema")]
    pub input_schema: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct McpToolResult {
    #[serde(default)]
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

fn response_id_matches(id: &Option<serde_json::Value>, expected_id: u64) -> bool {
    match id {
        Some(serde_json::Value::Number(n)) => n.as_u64() == Some(expected_id),
        Some(serde_json::Value::String(s)) => {
            s.parse::<u64>().ok() == Some(expected_id)
        }
        _ => false,
    }
}

#[derive(Debug, Clone)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
}

struct McpConnection {
    child: Child,
    stdin: tokio::process::ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
    next_id: u64,
    tools: Vec<McpToolInfo>,
}

impl McpConnection {
    async fn send_line(&mut self, line: &str) -> Result<(), ToolkitError> {
        self.stdin.write_all(line.as_bytes()).await.map_err(|e| {
            ToolkitError::ExecutionFailed(format!("Write error: {e}"))
        })?;
        self.stdin.write_all(b"\n").await.map_err(|e| {
            ToolkitError::ExecutionFailed(format!("Write error: {e}"))
        })?;
        self.stdin.flush().await.map_err(|e| {
            ToolkitError::ExecutionFailed(format!("Flush error: {e}"))
        })?;
        Ok(())
    }

    async fn send_notification(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<(), ToolkitError> {
        let notif = JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
        };
        let notif_json = serde_json::to_string(&notif).map_err(|e| {
            ToolkitError::ExecutionFailed(format!("Serialize error: {e}"))
        })?;
        debug!("MCP -> {}: {}", method, notif_json);
        self.send_line(&notif_json).await
    }

    async fn send_request(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, ToolkitError> {
        self.next_id += 1;
        let request_id = self.next_id;
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: request_id,
            method: method.to_string(),
            params,
        };
        let request_json = serde_json::to_string(&request).map_err(|e| {
            ToolkitError::ExecutionFailed(format!("Serialize error: {e}"))
        })?;

        debug!("MCP -> {}: {}", method, request_json);
        self.send_line(&request_json).await?;

        loop {
            let mut line = String::new();
            let bytes_read = tokio::time::timeout(
                MCP_RESPONSE_TIMEOUT,
                self.stdout.read_line(&mut line),
            )
            .await
            .map_err(|_| {
                ToolkitError::ExecutionFailed(format!(
                    "Timed out waiting for MCP response to '{}'",
                    method
                ))
            })?
            .map_err(|e| {
                ToolkitError::ExecutionFailed(format!("Read error: {e}"))
            })?;

            if bytes_read == 0 {
                return Err(ToolkitError::ExecutionFailed(
                    "MCP server closed stdout".to_string(),
                ));
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            debug!("MCP <- {}", trimmed);

            let parsed: serde_json::Value =
                serde_json::from_str(trimmed).map_err(|e| {
                    ToolkitError::ExecutionFailed(format!("Parse error: {e}"))
                })?;

            // Ignore notifications while waiting for our response.
            if parsed.get("method").is_some() && parsed.get("id").is_none() {
                continue;
            }

            let response: JsonRpcResponse =
                serde_json::from_value(parsed).map_err(|e| {
                    ToolkitError::ExecutionFailed(format!(
                        "Response decode error: {e}"
                    ))
                })?;

            if !response_id_matches(&response.id, request_id) {
                debug!(
                    "Ignoring MCP response with unmatched id while waiting for {}",
                    request_id
                );
                continue;
            }

            if let Some(err) = response.error {
                return Err(ToolkitError::ExecutionFailed(format!(
                    "MCP error {}: {}",
                    err.code, err.message
                )));
            }

            return Ok(response.result.unwrap_or(serde_json::Value::Null));
        }
    }

    async fn shutdown(&mut self) {
        let _ = self.child.kill().await;
    }
}

pub struct McpClient {
    connections: HashMap<String, Mutex<McpConnection>>,
}

impl McpClient {
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
        }
    }

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
            ToolkitError::ExecutionFailed(format!("Failed to spawn MCP server: {e}"))
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
                ToolkitError::ExecutionFailed(format!("Init parse error: {e}"))
            })?;

        conn.send_notification("notifications/initialized", None)
            .await?;

        let tools_result = conn.send_request("tools/list", None).await?;
        if let Some(tools_array) = tools_result.get("tools") {
            let tools: Vec<McpToolInfo> =
                serde_json::from_value(tools_array.clone()).map_err(|e| {
                    ToolkitError::ExecutionFailed(format!("Tools parse error: {e}"))
                })?;
            info!("Discovered {} tools from {}", tools.len(), config.name);
            conn.tools = tools;
        }

        self.connections
            .insert(config.name.clone(), Mutex::new(conn));
        Ok(())
    }

    fn get_connection(
        &self,
        server_name: &str,
    ) -> Result<&Mutex<McpConnection>, ToolkitError> {
        self.connections.get(server_name).ok_or_else(|| {
            ToolkitError::ToolNotFound(format!(
                "MCP server not connected: {}",
                server_name
            ))
        })
    }

    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<ToolResult, ToolkitError> {
        let conn_mutex = self.get_connection(server_name)?;
        let mut conn = conn_mutex.lock().await;
        let params = serde_json::json!({
            "name": tool_name,
            "arguments": arguments,
        });
        let result = conn.send_request("tools/call", Some(params)).await?;

        let tool_result: McpToolResult =
            serde_json::from_value(result).map_err(|e| {
                ToolkitError::ExecutionFailed(format!("Parse result error: {e}"))
            })?;
        let output = tool_result
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

    pub async fn list_resources(
        &self,
        server_name: &str,
    ) -> Result<serde_json::Value, ToolkitError> {
        let conn_mutex = self.get_connection(server_name)?;
        let mut conn = conn_mutex.lock().await;
        conn.send_request("resources/list", None).await
    }

    pub async fn read_resource(
        &self,
        server_name: &str,
        uri: &str,
    ) -> Result<serde_json::Value, ToolkitError> {
        let conn_mutex = self.get_connection(server_name)?;
        let mut conn = conn_mutex.lock().await;
        conn.send_request("resources/read", Some(serde_json::json!({ "uri": uri })))
            .await
    }

    pub async fn list_prompts(
        &self,
        server_name: &str,
    ) -> Result<serde_json::Value, ToolkitError> {
        let conn_mutex = self.get_connection(server_name)?;
        let mut conn = conn_mutex.lock().await;
        conn.send_request("prompts/list", None).await
    }

    pub async fn get_prompt(
        &self,
        server_name: &str,
        prompt_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, ToolkitError> {
        let conn_mutex = self.get_connection(server_name)?;
        let mut conn = conn_mutex.lock().await;
        conn.send_request(
            "prompts/get",
            Some(serde_json::json!({
                "name": prompt_name,
                "arguments": arguments,
            })),
        )
        .await
    }

    pub async fn disconnect_all(&mut self) {
        for (name, conn_mutex) in self.connections.drain() {
            info!("Disconnecting MCP server: {}", name);
            let mut conn = conn_mutex.into_inner();
            conn.shutdown().await;
        }
    }
}

pub struct McpToolkit {
    client: McpClient,
    tool_map: HashMap<String, (String, McpToolInfo)>,
}

impl McpToolkit {
    pub fn new() -> Self {
        Self {
            client: McpClient::new(),
            tool_map: HashMap::new(),
        }
    }

    fn make_prefixed_tool_name(server_name: &str, tool_name: &str) -> String {
        format!("mcp_{}_{}", server_name, tool_name)
    }

    fn virtual_tool(
        name: &str,
        description: &str,
        parameters: serde_json::Value,
    ) -> McpToolInfo {
        McpToolInfo {
            name: name.to_string(),
            description: Some(description.to_string()),
            input_schema: Some(parameters),
        }
    }

    pub async fn add_server(
        &mut self,
        config: McpServerConfig,
    ) -> Result<(), ToolkitError> {
        let server_name = config.name.clone();
        self.client.connect(config).await?;

        if let Some(conn_mutex) = self.client.connections.get(&server_name) {
            let conn = conn_mutex.lock().await;
            for tool in &conn.tools {
                let prefixed_name =
                    Self::make_prefixed_tool_name(&server_name, &tool.name);
                self.tool_map
                    .insert(prefixed_name, (server_name.clone(), tool.clone()));
            }
        }

        self.tool_map.insert(
            Self::make_prefixed_tool_name(&server_name, VIRTUAL_LIST_RESOURCES),
            (
                server_name.clone(),
                Self::virtual_tool(
                    VIRTUAL_LIST_RESOURCES,
                    "List resources exposed by this MCP server",
                    serde_json::json!({
                        "type": "object",
                        "properties": {}
                    }),
                ),
            ),
        );
        self.tool_map.insert(
            Self::make_prefixed_tool_name(&server_name, VIRTUAL_READ_RESOURCE),
            (
                server_name.clone(),
                Self::virtual_tool(
                    VIRTUAL_READ_RESOURCE,
                    "Read a resource from this MCP server",
                    serde_json::json!({
                        "type": "object",
                        "properties": {
                            "uri": { "type": "string" }
                        },
                        "required": ["uri"]
                    }),
                ),
            ),
        );
        self.tool_map.insert(
            Self::make_prefixed_tool_name(&server_name, VIRTUAL_LIST_PROMPTS),
            (
                server_name.clone(),
                Self::virtual_tool(
                    VIRTUAL_LIST_PROMPTS,
                    "List prompts exposed by this MCP server",
                    serde_json::json!({
                        "type": "object",
                        "properties": {}
                    }),
                ),
            ),
        );
        self.tool_map.insert(
            Self::make_prefixed_tool_name(&server_name, VIRTUAL_GET_PROMPT),
            (
                server_name.clone(),
                Self::virtual_tool(
                    VIRTUAL_GET_PROMPT,
                    "Get a prompt from this MCP server",
                    serde_json::json!({
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "arguments": { "type": "object" }
                        },
                        "required": ["name"]
                    }),
                ),
            ),
        );

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

        match info.name.as_str() {
            VIRTUAL_LIST_RESOURCES => {
                let value = self.client.list_resources(server_name).await?;
                Ok(ToolResult::success(value.to_string()))
            }
            VIRTUAL_READ_RESOURCE => {
                let uri =
                    args.get("uri").and_then(|v| v.as_str()).ok_or_else(|| {
                        ToolkitError::InvalidArguments("uri is required".to_string())
                    })?;
                let value = self.client.read_resource(server_name, uri).await?;
                Ok(ToolResult::success(value.to_string()))
            }
            VIRTUAL_LIST_PROMPTS => {
                let value = self.client.list_prompts(server_name).await?;
                Ok(ToolResult::success(value.to_string()))
            }
            VIRTUAL_GET_PROMPT => {
                let prompt_name =
                    args.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
                        ToolkitError::InvalidArguments(
                            "name is required".to_string(),
                        )
                    })?;
                let prompt_args = args
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));
                let value = self
                    .client
                    .get_prompt(server_name, prompt_name, prompt_args)
                    .await?;
                Ok(ToolResult::success(value.to_string()))
            }
            _ => self.client.call_tool(server_name, &info.name, args).await,
        }
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
        let json = serde_json::to_string(&req).expect("request serialization");
        assert!(json.contains("\"method\":\"test\""));
        assert!(json.contains("\"id\":1"));
    }

    #[test]
    fn test_response_id_matching() {
        assert!(response_id_matches(
            &Some(serde_json::Value::Number(1u64.into())),
            1
        ));
        assert!(response_id_matches(
            &Some(serde_json::Value::String("12".to_string())),
            12
        ));
        assert!(!response_id_matches(
            &Some(serde_json::Value::String("abc".to_string())),
            12
        ));
        assert!(!response_id_matches(
            &Some(serde_json::Value::Number(2u64.into())),
            1
        ));
    }

    #[test]
    fn test_mcp_toolkit_name() {
        let toolkit = McpToolkit::new();
        assert_eq!(toolkit.name(), "mcp");
    }
}
