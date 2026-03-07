//! OwnStack Agent — Native Rust AI Agent
//!
//! This crate provides the AI agent capabilities for OwnStack IDE, including:
//! - LLM providers (OpenRouter, Anthropic, Local/Ollama)
//! - Toolkits (core, LSP, Git, MCP, Healer, Multivers)
//! - Context window management
//! - Multi-agent orchestration (Planner, Critic, Worker)
//! - MCP server for exposing tools
//! - Project memory with priority rules engine
//! - Artifact extraction from LLM responses
//! - Structured telemetry and tracing

pub mod artifact_manager;
pub mod context;
pub mod index;
pub mod infra_sense;
pub mod lsp;
pub mod mcp_server;
pub mod mission;
pub mod orchestrator;
pub mod plugins;
pub mod policy_approval;
pub mod project_memory;
pub mod provider;
pub mod providers;
pub mod repomap;
pub mod resilience;
pub mod routing;
pub mod secret_store;
pub mod telemetry;
pub mod toolkits;

pub use artifact_manager::ArtifactManager;
pub use context::ContextManager;
pub use infra_sense::InfraSense;
pub use mcp_server::McpServer;
pub use mission::{
    MissionCompiler, MissionManager, MissionRecord, OpenClawOrchestrator,
};
pub use orchestrator::{AgentBudget, AgentOrchestrator, CriticResult, Mission};
pub use project_memory::{ProjectMemory, ProjectRules, RulesLoader};
pub use provider::{LlmMessage, LlmProvider, LlmResponse, Role, ToolCall};
pub use repomap::RepoMap;
pub use telemetry::{BlackBoxLogger, TokioLagMonitor, TraceContext};
