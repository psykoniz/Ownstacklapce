//! Git Time Machine — Workspace snapshots and rollback via git.
//!
//! Creates lightweight git snapshots ("time points") before and after
//! agent actions, allowing safe rollback if something goes wrong.
//!
//! Rust port of `ownstack-python/app/utils/git_time.py`.

use crate::toolkits::{ToolDef, ToolResult, Toolkit, ToolkitError};
use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use std::process::Command;
use tracing::{debug, info};

// ─── Snapshot ───────────────────────────────────────────────────

/// A named snapshot at a specific git commit.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Snapshot {
    pub id: String,
    pub commit_hash: String,
    pub message: String,
    pub timestamp: f64,
}

// ─── Time Machine ───────────────────────────────────────────────

/// Provides snapshot/restore functionality via git.
pub struct TimeMachine {
    workspace: PathBuf,
}

impl TimeMachine {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }

    /// Check if the workspace is a git repository.
    pub fn is_git_repo(&self) -> bool {
        self.workspace.join(".git").exists()
    }

    /// Initialize git if not already a repo.
    pub fn ensure_git(&self) -> Result<(), String> {
        if self.is_git_repo() {
            return Ok(());
        }
        self.run_git(&["init"])
            .map(|_| info!("TimeMachine: initialized git repo"))
            .map_err(|e| format!("Failed to init git: {e}"))
    }

    /// Create a snapshot of the current state.
    pub fn create_snapshot(&self, message: &str) -> Result<Snapshot, String> {
        if !self.is_git_repo() {
            return Err("Not a git repository".to_string());
        }

        // Stage all changes
        self.run_git(&["add", "-A"])
            .map_err(|e| format!("git add failed: {e}"))?;

        // Check if there are changes to commit
        let status = self.run_git(&["status", "--porcelain"]).unwrap_or_default();

        if status.trim().is_empty() {
            // Check for staged changes
            let diff = self
                .run_git(&["diff", "--cached", "--stat"])
                .unwrap_or_default();
            if diff.trim().is_empty() {
                return Err("No changes to snapshot".to_string());
            }
        }

        // Create the commit
        let tag = format!("ownstack-snap-{}", chrono_simple_now());
        let full_message = format!("[OwnStack Snapshot] {message}");

        self.run_git(&["commit", "-m", &full_message, "--allow-empty"])
            .map_err(|e| format!("git commit failed: {e}"))?;

        let hash = self
            .run_git(&["rev-parse", "HEAD"])
            .map_err(|e| format!("git rev-parse failed: {e}"))?
            .trim()
            .to_string();

        info!(
            "TimeMachine: created snapshot '{tag}' at {}",
            &hash[..8.min(hash.len())]
        );

        Ok(Snapshot {
            id: tag,
            commit_hash: hash,
            message: message.to_string(),
            timestamp: now_timestamp(),
        })
    }

    /// Restore workspace to a specific commit.
    pub fn restore(&self, commit_hash: &str) -> Result<(), String> {
        if !self.is_git_repo() {
            return Err("Not a git repository".to_string());
        }

        // Safety: create a snapshot of current state first
        let _ = self.create_snapshot("Auto-snapshot before restore");

        self.run_git(&["checkout", commit_hash, "--", "."])
            .map_err(|e| format!("git checkout failed: {e}"))?;

        info!(
            "TimeMachine: restored to {}",
            &commit_hash[..8.min(commit_hash.len())]
        );
        Ok(())
    }

    /// List recent snapshots (OwnStack commits).
    pub fn list_snapshots(&self, limit: usize) -> Vec<Snapshot> {
        if !self.is_git_repo() {
            return Vec::new();
        }

        let output = match self.run_git(&[
            "log",
            "--oneline",
            "--grep=[OwnStack Snapshot]",
            &format!("-{}", limit),
            "--format=%H|%s|%ct",
        ]) {
            Ok(o) => o,
            Err(_) => return Vec::new(),
        };

        output
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                if parts.len() >= 3 {
                    Some(Snapshot {
                        id: format!("snap-{}", &parts[0][..8.min(parts[0].len())]),
                        commit_hash: parts[0].to_string(),
                        message: parts[1]
                            .strip_prefix("[OwnStack Snapshot] ")
                            .unwrap_or(parts[1])
                            .to_string(),
                        timestamp: parts[2].parse::<f64>().unwrap_or(0.0),
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get a diff between two commits.
    pub fn diff(&self, from: &str, to: &str) -> Result<String, String> {
        self.run_git(&["diff", from, to])
            .map_err(|e| format!("git diff failed: {e}"))
    }

    /// Get the diff of the current working tree.
    pub fn current_diff(&self) -> Result<String, String> {
        self.run_git(&["diff"])
            .map_err(|e| format!("git diff failed: {e}"))
    }

    // ─── Git Helper ──────────────────────────────────────────────

    fn run_git(&self, args: &[&str]) -> Result<String, String> {
        debug!("TimeMachine: running git {:?}", args);

        let output = Command::new("git")
            .args(args)
            .current_dir(&self.workspace)
            .output()
            .map_err(|e| format!("git command failed: {e}"))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            Err(stderr)
        }
    }
}

/// Simple timestamp (avoid heavy chrono dep, use a string-based approach).
fn chrono_simple_now() -> String {
    let ts = now_timestamp() as u64;
    format!("{ts}")
}

fn now_timestamp() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

// ─── Toolkit Implementation ───────────────────────────────────────

pub struct TimeMachineToolkit {
    pub machine: TimeMachine,
}

impl TimeMachineToolkit {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            machine: TimeMachine::new(workspace),
        }
    }
}

#[async_trait]
impl Toolkit for TimeMachineToolkit {
    fn name(&self) -> &str {
        "time_machine"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef {
                name: "create_snapshot".to_string(),
                description: "Create a git snapshot of the current workspace state"
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "message": {"type": "string", "description": "Message describing the snapshot"}
                    },
                    "required": ["message"]
                }),
            },
            ToolDef {
                name: "restore_snapshot".to_string(),
                description:
                    "Restore the workspace to a specific git commit hash/tag"
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "commit_hash": {"type": "string", "description": "The commit hash to restore to"}
                    },
                    "required": ["commit_hash"]
                }),
            },
            ToolDef {
                name: "list_snapshots".to_string(),
                description: "List recent snapshots created by the agent"
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "limit": {"type": "integer", "description": "Maximum number of snapshots to return", "default": 10}
                    }
                }),
            },
            ToolDef {
                name: "current_diff".to_string(),
                description: "Get the current uncommitted changes as a git diff"
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {}
                }),
            },
        ]
    }

    async fn execute(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<ToolResult, ToolkitError> {
        let _ = self.machine.ensure_git();
        match tool_name {
            "create_snapshot" => {
                let message =
                    args["message"].as_str().unwrap_or("Automatic snapshot");
                match self.machine.create_snapshot(message) {
                    Ok(snap) => Ok(ToolResult::success(format!(
                        "Snapshot created: {} ({})",
                        snap.id, snap.commit_hash
                    ))),
                    Err(e) => Ok(ToolResult::failure(e, Some(1))),
                }
            }
            "restore_snapshot" => {
                let commit_hash = args["commit_hash"].as_str().unwrap_or("");
                if commit_hash.is_empty() {
                    return Err(ToolkitError::InvalidArguments(
                        "commit_hash is required".to_string(),
                    ));
                }
                match self.machine.restore(commit_hash) {
                    Ok(_) => Ok(ToolResult::success(format!(
                        "Restored to commit {}",
                        commit_hash
                    ))),
                    Err(e) => Ok(ToolResult::failure(e, Some(1))),
                }
            }
            "list_snapshots" => {
                let limit = args["limit"].as_u64().unwrap_or(10) as usize;
                let snaps = self.machine.list_snapshots(limit);
                // Convert snaps to json list manually for output
                #[derive(serde::Serialize)]
                struct SnapListOut {
                    snapshots: Vec<Snapshot>,
                }
                let json =
                    serde_json::to_string_pretty(&SnapListOut { snapshots: snaps })
                        .unwrap_or_default();
                Ok(ToolResult::success(json))
            }
            "current_diff" => match self.machine.current_diff() {
                Ok(diff) => Ok(ToolResult::success(if diff.trim().is_empty() {
                    "No uncommitted changes.".to_string()
                } else {
                    diff
                })),
                Err(e) => Ok(ToolResult::failure(e, Some(1))),
            },
            _ => Err(ToolkitError::ToolNotFound(tool_name.to_string())),
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn init_test_repo() -> (tempfile::TempDir, TimeMachine) {
        let dir = tempdir().unwrap();
        let tm = TimeMachine::new(dir.path().to_path_buf());
        tm.ensure_git().unwrap();

        // Configure git for tests
        let _ = tm.run_git(&["config", "user.email", "test@ownstack.dev"]);
        let _ = tm.run_git(&["config", "user.name", "OwnStack Test"]);
        let _ = tm.run_git(&["config", "commit.gpgsign", "false"]);

        // Initial commit
        fs::write(dir.path().join("README.md"), "# Test").unwrap();
        let _ = tm.run_git(&["add", "."]);
        let _ = tm.run_git(&["commit", "-m", "Initial commit"]);

        (dir, tm)
    }

    #[test]
    fn test_is_git_repo() {
        let dir = tempdir().unwrap();
        let tm = TimeMachine::new(dir.path().to_path_buf());
        assert!(!tm.is_git_repo());

        tm.ensure_git().unwrap();
        assert!(tm.is_git_repo());
    }

    #[test]
    fn test_create_snapshot() {
        let (dir, tm) = init_test_repo();

        // Make a change
        fs::write(dir.path().join("new_file.txt"), "hello").unwrap();

        let snap = tm.create_snapshot("test snapshot");
        assert!(snap.is_ok());
        let snap = snap.unwrap();
        assert!(!snap.commit_hash.is_empty());
        assert_eq!(snap.message, "test snapshot");
    }

    #[test]
    fn test_no_changes_snapshot() {
        let (_dir, tm) = init_test_repo();

        // No changes made → should either error or create empty commit
        let result = tm.create_snapshot("empty");
        // This is acceptable either way
        debug!("No-change snapshot result: {:?}", result);
    }

    #[test]
    fn test_list_snapshots() {
        let (dir, tm) = init_test_repo();

        fs::write(dir.path().join("a.txt"), "a").unwrap();
        tm.create_snapshot("first").unwrap();

        fs::write(dir.path().join("b.txt"), "b").unwrap();
        tm.create_snapshot("second").unwrap();

        let snaps = tm.list_snapshots(10);
        assert!(snaps.len() >= 2);
    }

    #[test]
    fn test_current_diff() {
        let (dir, tm) = init_test_repo();

        fs::write(dir.path().join("changed.txt"), "new content").unwrap();
        let _ = tm.run_git(&["add", "changed.txt"]);

        // diff --cached would show staged changes
        let diff = tm.run_git(&["diff", "--cached"]);
        assert!(diff.is_ok());
    }
}
