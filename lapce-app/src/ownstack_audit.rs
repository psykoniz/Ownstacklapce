use floem::reactive::{RwSignal, create_rw_signal};
use floem::prelude::{SignalGet, SignalUpdate};
use serde::{Deserialize, Serialize};

use crate::window_tab::CommonData;

/// A single audit log entry
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: String,
    pub action: AuditAction,
    pub command: String,
    pub policy_decision: String,
    pub success: bool,
    pub tool_name: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AuditAction {
    Exec,
    Read,
    Write,
    Delete,
    Search,
}

impl std::fmt::Display for AuditAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditAction::Exec => write!(f, "EXEC"),
            AuditAction::Read => write!(f, "READ"),
            AuditAction::Write => write!(f, "WRITE"),
            AuditAction::Delete => write!(f, "DELETE"),
            AuditAction::Search => write!(f, "SEARCH"),
        }
    }
}

/// Severity levels for filtering
#[derive(Clone, Debug, PartialEq)]
pub enum AuditSeverity {
    All,
    SecurityOnly,
    FailuresOnly,
}

/// OwnStack Audit Log Viewer Panel
#[derive(Clone)]
pub struct OwnStackAuditData {
    /// Whether the audit panel is visible
    pub visible: RwSignal<bool>,
    /// Audit log entries
    pub entries: RwSignal<Vec<AuditEntry>>,
    /// Current filter
    pub filter: RwSignal<AuditSeverity>,
    /// Search query
    pub search_query: RwSignal<String>,
    /// Max entries to display
    pub max_entries: RwSignal<usize>,
    #[allow(dead_code)]
    common: CommonData,
}

impl OwnStackAuditData {
    pub fn new(common: CommonData) -> Self {
        Self {
            visible: create_rw_signal(false),
            entries: create_rw_signal(Vec::new()),
            filter: create_rw_signal(AuditSeverity::All),
            search_query: create_rw_signal(String::new()),
            max_entries: create_rw_signal(500),
            common,
        }
    }

    /// Toggle audit panel visibility
    pub fn toggle(&self) {
        let current = self.visible.get_untracked();
        self.visible.set(!current);
    }

    /// Add an audit entry
    pub fn add_entry(&self, entry: AuditEntry) {
        self.entries.update(|entries| {
            entries.push(entry);
            // Keep only last N entries
            let max = self.max_entries.get_untracked();
            if entries.len() > max {
                let drain_count = entries.len() - max;
                entries.drain(0..drain_count);
            }
        });
    }

    /// Get filtered entries based on current filter and search
    pub fn filtered_entries(&self) -> Vec<AuditEntry> {
        let entries = self.entries.get_untracked();
        let filter = self.filter.get_untracked();
        let query = self.search_query.get_untracked().to_lowercase();

        entries
            .into_iter()
            .filter(|e| match filter {
                AuditSeverity::All => true,
                AuditSeverity::SecurityOnly => {
                    e.policy_decision == "Blocked" || e.policy_decision == "Ask"
                }
                AuditSeverity::FailuresOnly => !e.success,
            })
            .filter(|e| {
                if query.is_empty() {
                    true
                } else {
                    e.command.to_lowercase().contains(&query)
                        || e.action.to_string().to_lowercase().contains(&query)
                        || e.tool_name.as_ref().map_or(false, |t| {
                            t.to_lowercase().contains(&query)
                        })
                }
            })
            .collect()
    }

    /// Set the filter
    pub fn set_filter(&self, severity: AuditSeverity) {
        self.filter.set(severity);
    }

    /// Clear all entries
    pub fn clear(&self) {
        self.entries.set(Vec::new());
    }

    /// Get statistics
    pub fn stats(&self) -> AuditStats {
        let entries = self.entries.get_untracked();
        AuditStats {
            total: entries.len(),
            successes: entries.iter().filter(|e| e.success).count(),
            failures: entries.iter().filter(|e| !e.success).count(),
            blocked: entries
                .iter()
                .filter(|e| e.policy_decision == "Blocked")
                .count(),
        }
    }
}

/// Audit statistics summary
#[derive(Debug)]
pub struct AuditStats {
    pub total: usize,
    pub successes: usize,
    pub failures: usize,
    pub blocked: usize,
}
