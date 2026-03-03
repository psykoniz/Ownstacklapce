//! Mission System — Persistent mission lifecycle management.
//!
//! Provides:
//! - `models`: Mission data types with specs, modes, and checkpoints
//! - `manager`: Persistent mission manager with atomic saves and pub/sub
//! - `compiler`: Prompt-to-spec compilation via LLM
//! - `openclaw`: Multi-agent planning + execution + verification orchestrator

pub mod compiler;
pub mod manager;
pub mod models;
pub mod openclaw;

pub use compiler::MissionCompiler;
pub use manager::MissionManager;
pub use models::{
    Checkpoint, ExecutionStrategy, MissionEvent, MissionMode, MissionRecord,
    MissionSpec, MissionStatus, Permission,
};
pub use openclaw::OpenClawOrchestrator;
