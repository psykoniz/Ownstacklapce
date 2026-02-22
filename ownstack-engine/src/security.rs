//! Security Layer - Integrates Policy, Path Safety, and Audit
//!
//! This is the high-level API that all tools and agents should use
//! to ensure the multi-step security flow is followed correctly.

use chrono;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::{
    AuditEntry, AuditLogger, PathError, PathValidator, PolicyDecision, PolicyEngine,
    ToolResult,
};

pub struct SecurityContext {
    pub workspace: PathBuf,
    session_id: String,
}

impl SecurityContext {
    pub fn new(workspace: &Path) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
            session_id: Uuid::new_v4().to_string(),
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Evaluates a command and logs the result
    pub fn evaluate_and_audit(
        &self,
        command: &str,
        tool_name: &str,
    ) -> PolicyDecision {
        let decision = PolicyEngine::evaluate(command);

        // Always log the evaluation
        let entry = AuditEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id: self.session_id.clone(),
            action: "evaluate".to_string(),
            command: command.to_string(),
            policy_decision: decision.clone(),
            tool_name: tool_name.to_string(),
            success: true,
            duration_ms: 0,
            workspace: self.workspace.to_string_lossy().to_string(),
            paths_accessed: Vec::new(),
        };

        let logger = AuditLogger::new(self.workspace.clone());
        let _ = logger.log(entry);

        decision
    }

    /// Validates a list of paths
    pub fn validate_paths(&self, paths: &[String]) -> Result<(), PathError> {
        let validator = PathValidator::new(self.workspace.clone());
        for path in paths {
            validator.validate(Path::new(path))?;
        }
        Ok(())
    }

    /// Logs a tool execution result
    pub fn log_result(
        &self,
        command: &str,
        tool_name: &str,
        result: &ToolResult,
        duration_ms: u64,
    ) {
        let entry = AuditEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id: self.session_id.clone(),
            action: "exec".to_string(),
            command: command.to_string(),
            policy_decision: PolicyDecision::Auto, // If we reached here, it was approved
            tool_name: tool_name.to_string(),
            success: result.success,
            duration_ms,
            workspace: self.workspace.to_string_lossy().to_string(),
            paths_accessed: Vec::new(),
        };

        let logger = AuditLogger::new(self.workspace.clone());
        let _ = logger.log(entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_security_context_evaluate() {
        let dir = tempdir().unwrap();
        let ctx = SecurityContext::new(dir.path());

        let decision = ctx.evaluate_and_audit("ls", "test");
        assert_eq!(decision, PolicyDecision::Auto);
        assert_ne!(ctx.session_id(), "system");
        assert!(Uuid::parse_str(ctx.session_id()).is_ok());

        // Verify audit log exists
        let audit_path = dir.path().join(".ownstack").join("audit.jsonl");
        assert!(audit_path.exists());
    }

    #[test]
    fn test_security_context_validate_paths() {
        let dir = tempdir().unwrap();
        let ctx = SecurityContext::new(dir.path());

        assert!(ctx.validate_paths(&["src/lib.rs".to_string()]).is_ok());
        assert!(ctx.validate_paths(&["/etc/passwd".to_string()]).is_err());
    }
}
