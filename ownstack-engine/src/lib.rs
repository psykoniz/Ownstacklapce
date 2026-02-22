//! ownstack-engine: Security core for OwnStack Native IDE.
//!
//! This crate provides the PolicyEngine, PathValidator, AuditLogger,
//! and Sandbox abstractions required by the OwnStack security flow.

pub mod audit;
pub mod path_safety;
pub mod policy;
pub mod sandbox;
pub mod security;
pub mod tool_result;
pub mod vision;

pub use audit::{AuditEntry, AuditLogger};
pub use path_safety::{PathError, PathValidator};
pub use policy::{PolicyDecision, PolicyEngine};
pub use sandbox::process::ProcessSandbox;
pub use sandbox::{Sandbox, SandboxLevel};
pub use security::SecurityContext;
pub use tool_result::ToolResult;
