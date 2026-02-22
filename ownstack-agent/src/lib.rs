//! OwnStack Agent — Native Rust AI Agent
//!
//! This crate provides the AI agent capabilities for OwnStack IDE, including:
//! - LLM providers (OpenRouter, Anthropic, Local/Ollama)
//! - Toolkits (core, LSP, Git, MCP, Healer, Multivers)
//! - Context window management
//! - Multi-agent orchestration (Planner, Critic, Worker)
//! - MCP server for exposing tools
//! - Project memory (.ownstack/rules.md)

pub mod context;
pub mod index;
pub mod lsp;
pub mod mcp_server;
pub mod orchestrator;
pub mod plugins;
pub mod policy_approval;
pub mod project_memory;
pub mod provider;
pub mod providers;
pub mod resilience;
pub mod routing;
pub mod secret_store;
pub mod toolkits;

pub use context::ContextManager;
pub use mcp_server::McpServer;
pub use orchestrator::{AgentBudget, AgentOrchestrator, CriticResult, Mission};
pub use project_memory::ProjectMemory;
pub use provider::{LlmMessage, LlmProvider, LlmResponse, Role, ToolCall};
