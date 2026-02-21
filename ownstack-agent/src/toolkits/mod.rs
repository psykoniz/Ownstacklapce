//! Agent Toolkits
//!
//! Tools that the AI agent can use to interact with the codebase
//! and development environment.

pub mod core;
pub mod git;
pub mod healer;
pub mod lsp;
pub mod mcp;
pub mod multivers;
pub mod vision;

use std::sync::Arc;
use async_trait::async_trait;
use serde::Serialize;
use thiserror::Error;

pub use self::core::CoreToolkit;
pub use git::GitToolkit;
pub use healer::HealerToolkit;
pub use lsp::LspToolkit;
pub use mcp::McpToolkit;
pub use multivers::MultiversToolkit;
pub use vision::VisionToolkit;

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

pub use ownstack_engine::ToolResult;

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
    async fn execute(
        &self,
        args: serde_json::Value,
    ) -> Result<ToolResult, ToolkitError>;
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

    /// Verify toolkit signature (Phase 12 Secure Marketplace)
    fn verify(&self, _public_key: &[u8]) -> Result<(), ToolkitError> {
        Ok(()) // Default Ok for built-in toolkits
    }
}

pub struct SignedToolkit {
    pub toolkit: Arc<dyn Toolkit>,
    pub signature: Vec<u8>,
    pub public_key: Vec<u8>,
}

impl SignedToolkit {
    pub fn verify(&self) -> Result<(), ToolkitError> {
        use ed25519_dalek::{VerifyingKey, Signature, Verifier};
        
        let vk = VerifyingKey::from_bytes(&self.public_key.clone().try_into().map_err(|_| ToolkitError::SecurityViolation("Invalid public key".to_string()))?)
            .map_err(|e| ToolkitError::SecurityViolation(format!("Invalid public key: {}", e)))?;
        
        let sig = Signature::from_bytes(&self.signature.clone().try_into().map_err(|_| ToolkitError::SecurityViolation("Invalid signature length".to_string()))?);
        
        // In a real implementation, we would verify a hash of the toolkit binary or manifest
        // For this demonstration, we verify the toolkit name
        vk.verify(self.toolkit.name().as_bytes(), &sig)
            .map_err(|e| ToolkitError::SecurityViolation(format!("Signature verification failed: {}", e)))?;
        
        Ok(())
    }
}
