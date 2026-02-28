use std::sync::Arc;

use floem::{
    View,
    prelude::{SignalGet, SignalUpdate},
    reactive::{ReadSignal, RwSignal, create_rw_signal},
    style::{CursorStyle, Display},
    views::{
        Decorators, container, dyn_stack, h_stack, label, scroll, text, text_input,
        v_stack,
    },
};
use serde::{Deserialize, Serialize};

use crate::{
    config::{LapceConfig, color::LapceColor},
    window_tab::CommonData,
};

/// A single audit log entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditEntry {
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub policy_decision: String,
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub tool_name: String,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub paths_accessed: Vec<String>,
}

/// Severity levels for filtering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuditSeverity {
    All,
    SecurityOnly,
    FailuresOnly,
}

/// OwnStack Audit Log Viewer state.
#[derive(Clone)]
pub struct OwnStackAuditData {
    /// Whether the audit panel is visible.
    pub visible: RwSignal<bool>,
    /// Audit log entries.
    pub entries: RwSignal<Vec<AuditEntry>>,
    /// Current filter.
    pub filter: RwSignal<AuditSeverity>,
    /// Search query.
    pub search_query: RwSignal<String>,
    /// Max entries to display.
    pub max_entries: RwSignal<usize>,
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

    /// Toggle audit panel visibility.
    pub fn toggle(&self) {
        let current = self.visible.get_untracked();
        self.visible.set(!current);
    }

    /// Hide audit panel.
    pub fn hide(&self) {
        self.visible.set(false);
    }

    /// Add an audit entry.
    pub fn add_entry(&self, entry: AuditEntry) {
        self.entries.update(|entries| {
            entries.push(entry);
            let max = self.max_entries.get_untracked();
            if entries.len() > max {
                let drain_count = entries.len() - max;
                entries.drain(0..drain_count);
            }
        });
    }

    /// Parse and append one audit event emitted through RPC.
    pub fn add_entry_from_json(&self, json_entry: &str) {
        if let Some(entry) = parse_audit_entry(json_entry) {
            self.add_entry(entry);
        }
    }

    /// Reload entries from `.ownstack/audit.jsonl`.
    pub fn reload_from_disk(&self) {
        let Some(workspace_root) = self.common.workspace.path.as_ref() else {
            return;
        };
        let path = workspace_root.join(".ownstack").join("audit.jsonl");
        let Ok(content) = std::fs::read_to_string(path) else {
            return;
        };

        let mut loaded = Vec::new();
        for line in content.lines() {
            if let Some(entry) = parse_audit_entry(line) {
                loaded.push(entry);
            }
        }

        let max = self.max_entries.get_untracked();
        if loaded.len() > max {
            let start = loaded.len() - max;
            loaded = loaded[start..].to_vec();
        }
        self.entries.set(loaded);
    }

    /// Get filtered entries based on current filter and search.
    pub fn filtered_entries(&self) -> Vec<AuditEntry> {
        let entries = self.entries.get_untracked();
        let filter = self.filter.get_untracked();
        let query = self.search_query.get_untracked().to_lowercase();

        entries
            .into_iter()
            .filter(|e| match filter {
                AuditSeverity::All => true,
                AuditSeverity::SecurityOnly => {
                    let decision = e.policy_decision.to_ascii_lowercase();
                    decision == "blocked" || decision == "ask"
                }
                AuditSeverity::FailuresOnly => !e.success,
            })
            .filter(|e| {
                if query.is_empty() {
                    true
                } else {
                    e.command.to_lowercase().contains(&query)
                        || e.action.to_lowercase().contains(&query)
                        || e.tool_name.to_lowercase().contains(&query)
                }
            })
            .collect()
    }

    /// Set the filter.
    pub fn set_filter(&self, severity: AuditSeverity) {
        self.filter.set(severity);
    }

    /// Clear all entries.
    pub fn clear(&self) {
        self.entries.set(Vec::new());
    }

    /// Get statistics.
    pub fn stats(&self) -> AuditStats {
        let entries = self.entries.get_untracked();
        AuditStats {
            total: entries.len(),
            successes: entries.iter().filter(|e| e.success).count(),
            failures: entries.iter().filter(|e| !e.success).count(),
            blocked: entries
                .iter()
                .filter(|e| e.policy_decision.eq_ignore_ascii_case("blocked"))
                .count(),
        }
    }
}

/// Audit statistics summary.
#[derive(Debug)]
pub struct AuditStats {
    pub total: usize,
    pub successes: usize,
    pub failures: usize,
    pub blocked: usize,
}

fn parse_audit_entry(line: &str) -> Option<AuditEntry> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;

    Some(AuditEntry {
        timestamp: value
            .get("timestamp")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        session_id: value
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        action: value
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        command: value
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        policy_decision: value
            .get("policy_decision")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        success: value
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        tool_name: value
            .get("tool_name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        duration_ms: value
            .get("duration_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        paths_accessed: value
            .get("paths_accessed")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(ToString::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
    })
}

pub fn ownstack_audit_overlay(
    data: OwnStackAuditData,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let query = data.search_query;
    let stats_data = data.clone();
    let list_data = data.clone();
    let visible_data = data.clone();

    container(
        container(
            v_stack((
                h_stack((
                    label(|| "OwnStack Audit".to_string()).style(move |s| {
                        s.font_bold()
                            .font_size(13.0)
                            .color(config.get().color(LapceColor::EDITOR_FOREGROUND))
                    }),
                    label(move || {
                        let stats = stats_data.stats();
                        format!(
                            "total:{} ok:{} fail:{} blocked:{}",
                            stats.total,
                            stats.successes,
                            stats.failures,
                            stats.blocked
                        )
                    })
                    .style(move |s| {
                        s.margin_left(10.0)
                            .font_size(11.0)
                            .color(config.get().color(LapceColor::EDITOR_DIM))
                    }),
                    label(|| "All".to_string())
                        .on_click_stop({
                            let data = data.clone();
                            move |_| data.set_filter(AuditSeverity::All)
                        })
                        .style(move |s| {
                            s.padding_horiz(8.0)
                                .padding_vert(4.0)
                                .margin_left(8.0)
                                .border(1.0)
                                .border_radius(4.0)
                                .border_color(
                                    config.get().color(LapceColor::LAPCE_BORDER),
                                )
                                .cursor(CursorStyle::Pointer)
                        }),
                    label(|| "Security".to_string())
                        .on_click_stop({
                            let data = data.clone();
                            move |_| data.set_filter(AuditSeverity::SecurityOnly)
                        })
                        .style(move |s| {
                            s.padding_horiz(8.0)
                                .padding_vert(4.0)
                                .margin_left(6.0)
                                .border(1.0)
                                .border_radius(4.0)
                                .border_color(
                                    config.get().color(LapceColor::LAPCE_BORDER),
                                )
                                .cursor(CursorStyle::Pointer)
                        }),
                    label(|| "Failures".to_string())
                        .on_click_stop({
                            let data = data.clone();
                            move |_| data.set_filter(AuditSeverity::FailuresOnly)
                        })
                        .style(move |s| {
                            s.padding_horiz(8.0)
                                .padding_vert(4.0)
                                .margin_left(6.0)
                                .border(1.0)
                                .border_radius(4.0)
                                .border_color(
                                    config.get().color(LapceColor::LAPCE_BORDER),
                                )
                                .cursor(CursorStyle::Pointer)
                        }),
                    label(|| "Reload".to_string())
                        .on_click_stop({
                            let data = data.clone();
                            move |_| data.reload_from_disk()
                        })
                        .style(move |s| {
                            s.padding_horiz(8.0)
                                .padding_vert(4.0)
                                .margin_left(8.0)
                                .border(1.0)
                                .border_radius(4.0)
                                .border_color(
                                    config.get().color(LapceColor::LAPCE_BORDER),
                                )
                                .cursor(CursorStyle::Pointer)
                        }),
                    label(|| "Clear".to_string())
                        .on_click_stop({
                            let data = data.clone();
                            move |_| data.clear()
                        })
                        .style(move |s| {
                            s.padding_horiz(8.0)
                                .padding_vert(4.0)
                                .margin_left(6.0)
                                .border(1.0)
                                .border_radius(4.0)
                                .border_color(
                                    config.get().color(LapceColor::LAPCE_BORDER),
                                )
                                .cursor(CursorStyle::Pointer)
                        }),
                    label(|| "Close".to_string())
                        .on_click_stop({
                            let data = data.clone();
                            move |_| data.hide()
                        })
                        .style(move |s| {
                            s.padding_horiz(8.0)
                                .padding_vert(4.0)
                                .margin_left(6.0)
                                .border(1.0)
                                .border_radius(4.0)
                                .border_color(
                                    config.get().color(LapceColor::LAPCE_BORDER),
                                )
                                .cursor(CursorStyle::Pointer)
                        }),
                ))
                .style(|s| s.items_center().width_full()),
                text_input(query)
                    .placeholder("Filter by command / action / tool")
                    .style(move |s| {
                        let config = config.get();
                        s.width_full()
                            .margin_top(10.0)
                            .padding(8.0)
                            .border(1.0)
                            .border_radius(4.0)
                            .border_color(config.color(LapceColor::LAPCE_BORDER))
                            .background(config.color(LapceColor::EDITOR_BACKGROUND))
                    }),
                // Empty state when no entries
                crate::ownstack_empty_state::audit_empty_state()
                    .style(move |s| {
                        let has_entries = !list_data.entries.get().is_empty();
                        s.apply_if(has_entries, |s| s.hide())
                    }),
                scroll(
                    dyn_stack(
                        move || list_data.filtered_entries(),
                        |entry| {
                            (
                                entry.timestamp.clone(),
                                entry.command.clone(),
                                entry.duration_ms,
                            )
                        },
                        move |entry| {
                            v_stack((
                                h_stack((
                                    text(format!(
                                        "{} | {} | {}",
                                        entry.timestamp,
                                        entry.action,
                                        if entry.success { "ok" } else { "fail" }
                                    ))
                                    .style(
                                        move |s| {
                                            let color = if entry.success {
                                                config.get().color(
                                                    LapceColor::EDITOR_FOREGROUND,
                                                )
                                            } else {
                                                config
                                                    .get()
                                                    .color(LapceColor::LAPCE_ERROR)
                                            };
                                            s.font_size(11.0).color(color)
                                        },
                                    ),
                                    text(format!(
                                        "policy:{} | {}ms",
                                        entry.policy_decision, entry.duration_ms
                                    ))
                                    .style(
                                        move |s| {
                                            s.margin_left(8.0).font_size(10.0).color(
                                                config
                                                    .get()
                                                    .color(LapceColor::EDITOR_DIM),
                                            )
                                        },
                                    ),
                                ))
                                .style(|s| s.items_center().width_full()),
                                text(entry.command).style(move |s| {
                                    s.width_full()
                                        .margin_top(4.0)
                                        .font_size(12.0)
                                        .color(
                                            config.get().color(
                                                LapceColor::EDITOR_FOREGROUND,
                                            ),
                                        )
                                }),
                            ))
                            .style(move |s| {
                                s.width_full()
                                    .padding(8.0)
                                    .margin_top(6.0)
                                    .border(1.0)
                                    .border_radius(4.0)
                                    .border_color(
                                        config.get().color(LapceColor::LAPCE_BORDER),
                                    )
                                    .background(
                                        config
                                            .get()
                                            .color(LapceColor::PANEL_BACKGROUND),
                                    )
                            })
                        },
                    )
                    .style(|s| s.flex_col().width_full()),
                )
                .style(|s| s.width_full().margin_top(10.0).max_height(420.0)),
            ))
            .style(move |s| {
                let config = config.get();
                s.width_full()
                    .padding(12.0)
                    .border(1.0)
                    .border_radius(6.0)
                    .border_color(config.color(LapceColor::LAPCE_BORDER))
                    .background(config.color(LapceColor::PALETTE_BACKGROUND))
                    .pointer_events_auto()
            }),
        )
        .style(|s| {
            s.width(720.0)
                .max_width_pct(92.0)
                .margin_top(48.0)
                .margin_right(12.0)
        }),
    )
    .on_event_stop(floem::event::EventListener::PointerDown, |_| {})
    .style(move |s| {
        s.display(if visible_data.visible.get() {
            Display::Flex
        } else {
            Display::None
        })
        .absolute()
        .size_full()
        .justify_end()
        .items_start()
        .pointer_events_none()
    })
}

#[cfg(test)]
mod tests {
    use super::parse_audit_entry;

    #[test]
    fn parse_audit_entry_from_engine_json() {
        let raw = r#"{"timestamp":"2026-02-23T00:00:00Z","session_id":"s1","action":"exec","command":"cargo check","policy_decision":"Auto","tool_name":"core.exec","success":true,"duration_ms":12,"workspace":"w","paths_accessed":["Cargo.toml"]}"#;
        let entry = parse_audit_entry(raw).expect("entry");
        assert_eq!(entry.action, "exec");
        assert_eq!(entry.command, "cargo check");
        assert!(entry.success);
        assert_eq!(entry.duration_ms, 12);
    }
}
