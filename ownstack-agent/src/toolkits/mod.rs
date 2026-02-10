//! Agent Toolkits
//!
//! Tools that the AI agent can use to interact with the codebase
//! and development environment.

pub mod core;
pub mod lsp;
pub mod mcp;
pub mod git;
pub mod healer;
pub mod multivers;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use self::core::CoreToolkit;
pub use lsp::LspToolkit;
pub use mcp::McpToolkit;
pub use git::GitToolkit;
pub use healer::HealerToolkit;
pub use multivers::MultiversToolkit;

/// Errors from toolkit operations
#[derive(Error, Debug)]
pub enum ToolkitError {
    #[error("Tool not found: {0}")]
    ToolNotFound(String),
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Security violation: {0}")]
    SecurityViolation(String),
    #[error("Invalid arguments: {0}")]
    InvalidArguments(String),
}

/// Result of a tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ToolResult {
    pub fn success(output: impl Into<String>) -> Self {
        Self {
            success: true,
            output: output.into(),
            error: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        let msg = message.into();
        Self {
            success: false,
            output: String::new(),
            error: Some(msg),
        }
    }
}

/// Definition of a tool for the LLM
#[derive(Debug, Clone, Serialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Trait for tool implementations
#[async_trait]
pub trait Tool: Send + Sync {
    /// Get the tool definition
    fn definition(&self) -> ToolDef;

    /// Execute the tool with given arguments
    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, ToolkitError>;
}

/// Trait for toolkit implementations
#[async_trait]
pub trait Toolkit: Send + Sync {
    /// Get the toolkit name
    fn name(&self) -> &str;

    /// Get all tool definitions from this toolkit
    fn tools(&self) -> Vec<ToolDef>;

    /// Execute a tool by name
    async fn execute(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<ToolResult, ToolkitError>;
}
