//! OwnStack Agent — Native Rust AI Agent
//!
//! This crate provides the AI agent capabilities for OwnStack IDE, including:
//! - LLM providers (OpenRouter, Anthropic, Local/Ollama)
//! - Toolkits (core, LSP, Git, MCP, Healer, Multivers)
//! - Context window management
//! - Multi-agent orchestration (Planner, Critic, Worker)
//! - MCP server for exposing tools
//! - Project memory (.ownstack/rules.md)

pub mod provider;
pub mod providers;
pub mod context;
pub mod toolkits;
pub mod orchestrator;
pub mod mcp_server;
pub mod project_memory;
pub mod resilience;
pub mod lsp;

pub use provider::{LlmProvider, LlmMessage, LlmResponse, ToolCall, Role};
pub use context::ContextManager;
pub use orchestrator::{AgentOrchestrator, AgentBudget, Mission, CriticResult};
pub use mcp_server::McpServer;
pub use project_memory::ProjectMemory;

