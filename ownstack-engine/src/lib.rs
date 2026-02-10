//! ownstack-engine: Security core for OwnStack Native IDE.
//! 
//! This crate provides the PolicyEngine, PathValidator, AuditLogger, 
//! and Sandbox abstractions required by the OwnStack security flow.

pub mod policy;
pub mod path_safety;
pub mod audit;
pub mod tool_result;
pub mod sandbox;
pub mod security;

pub use policy::{PolicyEngine, PolicyDecision};
pub use path_safety::{PathValidator, PathError};
pub use audit::{AuditLogger, AuditEntry};
pub use tool_result::ToolResult;
pub use sandbox::{Sandbox, SandboxLevel};
pub use sandbox::process::ProcessSandbox;
pub use security::SecurityContext;
