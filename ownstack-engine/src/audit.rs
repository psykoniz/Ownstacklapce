use crate::policy::PolicyDecision;
use chrono::Utc;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: String,
    pub session_id: String,
    pub action: String,
    pub command: String,
    pub policy_decision: PolicyDecision,
    pub tool_name: String,
    pub success: bool,
    pub duration_ms: u64,
    pub workspace: String,
    pub paths_accessed: Vec<String>,
}

pub struct AuditLogger {
    log_path: PathBuf,
}

impl AuditLogger {
    pub fn new(workspace: PathBuf) -> Self {
        let mut log_path = workspace;
        log_path.push(".ownstack");
        log_path.push("audit.jsonl");

        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        Self { log_path }
    }

    /// Logs an entry to the audit file in JSONL format.
    ///
    /// Writes are protected by an OS-level exclusive lock so multiple
    /// processes/threads cannot interleave JSON lines.
    pub fn log(&self, mut entry: AuditEntry) -> std::io::Result<()> {
        entry.timestamp = Utc::now().to_rfc3339();

        let json = serde_json::to_string(&entry).map_err(std::io::Error::other)?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&self.log_path)?;

        file.lock_exclusive()?;

        let write_result = (|| -> std::io::Result<()> {
            writeln!(file, "{}", json)?;
            file.flush()?;
            Ok(())
        })();

        let unlock_result = file.unlock();

        match (write_result, unlock_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(write_err), _) => Err(write_err),
            (Ok(()), Err(unlock_err)) => Err(unlock_err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_workspace(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("audit_test_{}", name));
        let _ = fs::remove_dir_all(&p);
        let _ = fs::create_dir_all(&p);
        p
    }

    fn make_entry(action: &str, decision: PolicyDecision) -> AuditEntry {
        AuditEntry {
            timestamp: String::new(),
            session_id: "test-session".to_string(),
            action: action.to_string(),
            command: format!("test_{}", action),
            policy_decision: decision,
            tool_name: "test-tool".to_string(),
            success: true,
            duration_ms: 42,
            workspace: "/test".to_string(),
            paths_accessed: vec!["file.rs".to_string()],
        }
    }

    #[test]
    fn test_log_creates_file() {
        let ws = temp_workspace("create");
        let logger = AuditLogger::new(ws.clone());
        let entry = make_entry("read", PolicyDecision::Auto);
        assert!(logger.log(entry).is_ok());

        let log_file = ws.join(".ownstack").join("audit.jsonl");
        assert!(log_file.exists());
        let _ = fs::remove_dir_all(ws);
    }

    #[test]
    fn test_log_appends_not_overwrites() {
        let ws = temp_workspace("append");
        let logger = AuditLogger::new(ws.clone());

        logger
            .log(make_entry("first", PolicyDecision::Auto))
            .unwrap();
        logger
            .log(make_entry("second", PolicyDecision::Ask))
            .unwrap();
        logger
            .log(make_entry("third", PolicyDecision::Blocked))
            .unwrap();

        let content =
            fs::read_to_string(ws.join(".ownstack").join("audit.jsonl")).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3);
        let _ = fs::remove_dir_all(ws);
    }

    #[test]
    fn test_log_valid_json_per_line() {
        let ws = temp_workspace("json");
        let logger = AuditLogger::new(ws.clone());

        logger
            .log(make_entry("exec", PolicyDecision::Auto))
            .unwrap();
        logger
            .log(make_entry("write", PolicyDecision::Ask))
            .unwrap();

        let content =
            fs::read_to_string(ws.join(".ownstack").join("audit.jsonl")).unwrap();
        for line in content.lines() {
            let parsed: serde_json::Value =
                serde_json::from_str(line).expect("Each line must be valid JSON");
            assert!(parsed.get("action").is_some());
            assert!(parsed.get("timestamp").is_some());
            assert!(parsed.get("session_id").is_some());
        }
        let _ = fs::remove_dir_all(ws);
    }

    #[test]
    fn test_timestamp_is_set() {
        let ws = temp_workspace("timestamp");
        let logger = AuditLogger::new(ws.clone());
        logger
            .log(make_entry("exec", PolicyDecision::Auto))
            .unwrap();

        let content =
            fs::read_to_string(ws.join(".ownstack").join("audit.jsonl")).unwrap();
        let line = content.lines().next().unwrap();
        let parsed: AuditEntry = serde_json::from_str(line).unwrap();
        assert!(
            !parsed.timestamp.is_empty(),
            "timestamp should be populated"
        );
        let _ = fs::remove_dir_all(ws);
    }

    #[test]
    fn test_audit_entry_serialization_roundtrip() {
        let entry = AuditEntry {
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            session_id: "sess-123".to_string(),
            action: "exec".to_string(),
            command: "cargo test".to_string(),
            policy_decision: PolicyDecision::Auto,
            tool_name: "core.exec".to_string(),
            success: true,
            duration_ms: 1234,
            workspace: "/home/user/project".to_string(),
            paths_accessed: vec![
                "src/main.rs".to_string(),
                "Cargo.toml".to_string(),
            ],
        };

        let json = serde_json::to_string(&entry).unwrap();
        let back: AuditEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.session_id, "sess-123");
        assert_eq!(back.action, "exec");
        assert_eq!(back.command, "cargo test");
        assert_eq!(back.policy_decision, PolicyDecision::Auto);
        assert!(back.success);
        assert_eq!(back.duration_ms, 1234);
        assert_eq!(back.paths_accessed.len(), 2);
    }

    #[test]
    fn test_audit_entry_with_all_decisions() {
        for decision in [
            PolicyDecision::Auto,
            PolicyDecision::Ask,
            PolicyDecision::Blocked,
        ] {
            let entry = make_entry("test", decision.clone());
            let json = serde_json::to_string(&entry).unwrap();
            let back: AuditEntry = serde_json::from_str(&json).unwrap();
            assert_eq!(back.policy_decision, decision);
        }
    }

    #[test]
    fn test_audit_entry_with_empty_paths() {
        let entry = AuditEntry {
            timestamp: String::new(),
            session_id: "s".to_string(),
            action: "read".to_string(),
            command: "cat file".to_string(),
            policy_decision: PolicyDecision::Auto,
            tool_name: "core".to_string(),
            success: true,
            duration_ms: 0,
            workspace: "/w".to_string(),
            paths_accessed: vec![],
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"paths_accessed\":[]"));
    }

    #[test]
    fn test_audit_entry_failure() {
        let entry = AuditEntry {
            timestamp: String::new(),
            session_id: "s".to_string(),
            action: "exec".to_string(),
            command: "bad command".to_string(),
            policy_decision: PolicyDecision::Blocked,
            tool_name: "core".to_string(),
            success: false,
            duration_ms: 0,
            workspace: "/w".to_string(),
            paths_accessed: vec![],
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"success\":false"));
    }

    #[test]
    fn test_logger_creates_directory() {
        let ws = temp_workspace("mkdir");
        let _ = fs::remove_dir_all(&ws);
        let _logger = AuditLogger::new(ws.clone());
        assert!(ws.join(".ownstack").exists());
        let _ = fs::remove_dir_all(ws);
    }

    #[test]
    fn stress_test_500_entries() {
        let ws = temp_workspace("stress500");
        let logger = AuditLogger::new(ws.clone());

        for i in 0..500 {
            let entry = make_entry(&format!("action_{}", i), PolicyDecision::Auto);
            logger.log(entry).unwrap();
        }

        let content =
            fs::read_to_string(ws.join(".ownstack").join("audit.jsonl")).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 500);
        let _ = fs::remove_dir_all(ws);
    }

    #[test]
    fn stress_test_large_entry() {
        let ws = temp_workspace("large");
        let logger = AuditLogger::new(ws.clone());

        let entry = AuditEntry {
            timestamp: String::new(),
            session_id: "s".to_string(),
            action: "exec".to_string(),
            command: "a".repeat(50000),
            policy_decision: PolicyDecision::Auto,
            tool_name: "tool".to_string(),
            success: true,
            duration_ms: 0,
            workspace: "/w".to_string(),
            paths_accessed: (0..1000).map(|i| format!("file_{}.rs", i)).collect(),
        };
        assert!(logger.log(entry).is_ok());
        let _ = fs::remove_dir_all(ws);
    }

    #[test]
    fn stress_test_concurrent_logging() {
        use std::sync::Arc;
        use std::thread;

        let ws = temp_workspace("concurrent");
        let log_path = ws.join(".ownstack").join("audit.jsonl");
        let ws_arc = Arc::new(ws.clone());

        let handles: Vec<_> = (0..20)
            .map(|i| {
                let ws = Arc::clone(&ws_arc);
                thread::spawn(move || {
                    let logger = AuditLogger::new(ws.as_ref().clone());
                    for j in 0..10 {
                        let entry = make_entry(
                            &format!("thread_{}_{}", i, j),
                            PolicyDecision::Auto,
                        );
                        logger.log(entry).unwrap();
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        let content = fs::read_to_string(&log_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 200, "concurrent writes must keep all entries");

        for line in lines {
            let _: AuditEntry = serde_json::from_str(line)
                .expect("each audit line must remain valid JSON");
        }

        let _ = fs::remove_dir_all(ws);
    }
}
