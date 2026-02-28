use floem::prelude::*;
use floem::reactive::RwSignal;
use floem::{
    peniko::Color,
    style::{CursorStyle, Style},
    text::Weight,
    views::{dyn_stack, h_stack, label, scroll, v_stack},
    View,
};
use std::rc::Rc;

use crate::{
    app::clickable_icon,
    config::icon::LapceIcons,
    window_tab::{CommonData, WindowTabData},
};

// ─── Data model ──────────────────────────────────────────────────────────────

/// Status of a single MCP server entry.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum McpServerStatus {
    /// Command binary found in PATH — likely connectable.
    Available,
    /// Command binary not found — server can't start.
    CommandNotFound,
    /// Config loaded but not yet probed.
    Unknown,
}

impl McpServerStatus {
    fn label(&self) -> &'static str {
        match self {
            Self::Available => "Available",
            Self::CommandNotFound => "Command not found",
            Self::Unknown => "Unknown",
        }
    }

    fn color(&self) -> Color {
        match self {
            Self::Available => Color::from_rgb8(52, 211, 153),
            Self::CommandNotFound => Color::from_rgb8(248, 113, 113),
            Self::Unknown => Color::from_rgb8(250, 204, 21),
        }
    }
}

#[derive(Clone)]
pub struct McpServerInfo {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub status: McpServerStatus,
    /// Human-readable path to the config file this entry came from.
    pub config_source: String,
}

// ─── Config file shapes (mirrors ownstack-agent/src/main.rs) ─────────────────

#[derive(serde::Deserialize)]
struct McpServersFile {
    #[serde(default)]
    servers: Vec<McpServerEntry>,
}

#[derive(serde::Deserialize)]
struct McpServerEntry {
    name: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default = "default_enabled")]
    enabled: bool,
}

fn default_enabled() -> bool {
    true
}

/// Claude Desktop config shape (subset we need).
#[derive(serde::Deserialize)]
struct ClaudeDesktopConfig {
    #[serde(rename = "mcpServers", default)]
    mcp_servers: std::collections::HashMap<String, ClaudeDesktopServer>,
}

#[derive(serde::Deserialize)]
struct ClaudeDesktopServer {
    command: String,
    #[serde(default)]
    args: Vec<String>,
}

// ─── Config discovery ─────────────────────────────────────────────────────────

/// Returns the platform-specific Claude Desktop config path.
fn claude_desktop_config_path() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").ok()?;
        Some(
            std::path::PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("Claude")
                .join("claude_desktop_config.json"),
        )
    }
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").ok()?;
        Some(
            std::path::PathBuf::from(appdata)
                .join("Claude")
                .join("claude_desktop_config.json"),
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        // Linux / others: XDG_CONFIG_HOME or ~/.config
        let config_home = std::env::var("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_default();
                std::path::PathBuf::from(home).join(".config")
            });
        Some(config_home.join("Claude").join("claude_desktop_config.json"))
    }
}

/// Returns `true` if the command can be found in PATH (or is an absolute path).
fn command_in_path(cmd: &str) -> bool {
    let p = std::path::Path::new(cmd);
    if p.is_absolute() {
        return p.exists();
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            if dir.join(cmd).exists() {
                return true;
            }
        }
    }
    false
}

/// Load MCP server entries from all known config sources.
/// Returns (servers, searched_paths).
fn load_mcp_servers(workspace: Option<&std::path::Path>) -> (Vec<McpServerInfo>, Vec<String>) {
    let mut results: Vec<McpServerInfo> = Vec::new();
    let mut searched: Vec<String> = Vec::new();

    // 1. Workspace-local config: <ws>/.ownstack/mcp_servers.json
    if let Some(ws) = workspace {
        let ws_path = ws.join(".ownstack").join("mcp_servers.json");
        let ws_str = ws_path.display().to_string();
        searched.push(ws_str.clone());
        if let Ok(content) = std::fs::read_to_string(&ws_path) {
            if let Ok(parsed) = serde_json::from_str::<McpServersFile>(&content) {
                for entry in parsed.servers.into_iter().filter(|e| e.enabled) {
                    let status = if command_in_path(&entry.command) {
                        McpServerStatus::Available
                    } else {
                        McpServerStatus::CommandNotFound
                    };
                    results.push(McpServerInfo {
                        name: entry.name,
                        command: entry.command,
                        args: entry.args,
                        status,
                        config_source: ws_str.clone(),
                    });
                }
            }
        }
    }

    // 2. Claude Desktop global config
    if let Some(cd_path) = claude_desktop_config_path() {
        let cd_str = cd_path.display().to_string();
        searched.push(cd_str.clone());
        if let Ok(content) = std::fs::read_to_string(&cd_path) {
            if let Ok(parsed) = serde_json::from_str::<ClaudeDesktopConfig>(&content) {
                let mut servers: Vec<_> = parsed.mcp_servers.into_iter().collect();
                // Sort alphabetically for deterministic display
                servers.sort_by(|a, b| a.0.cmp(&b.0));
                for (name, server) in servers {
                    if results.iter().any(|r| r.name == name) {
                        continue; // skip duplicates
                    }
                    let status = if command_in_path(&server.command) {
                        McpServerStatus::Available
                    } else {
                        McpServerStatus::CommandNotFound
                    };
                    results.push(McpServerInfo {
                        name,
                        command: server.command,
                        args: server.args,
                        status,
                        config_source: cd_str.clone(),
                    });
                }
            }
        }
    }

    (results, searched)
}

// ─── OwnStackMcpData ─────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct OwnStackMcpData {
    pub common: Rc<CommonData>,
    pub servers: RwSignal<Vec<McpServerInfo>>,
    /// Non-None when no servers are configured — explains what was searched.
    pub no_config_message: RwSignal<Option<String>>,
}

impl OwnStackMcpData {
    pub fn new(common: Rc<CommonData>, workspace: Option<std::path::PathBuf>) -> Self {
        let cx = common.scope;
        let (loaded_servers, searched) = load_mcp_servers(workspace.as_deref());

        let no_config_message = if loaded_servers.is_empty() {
            let paths = searched.join(", ");
            cx.create_rw_signal(Some(format!(
                "No MCP servers configured. Searched: {}",
                if paths.is_empty() {
                    "(no workspace)".to_string()
                } else {
                    paths
                }
            )))
        } else {
            cx.create_rw_signal(None)
        };

        let servers = cx.create_rw_signal(loaded_servers);
        Self {
            common,
            servers,
            no_config_message,
        }
    }
}

// ─── Panel UI ─────────────────────────────────────────────────────────────────

pub fn mcp_panel(
    window_tab_data: Rc<WindowTabData>,
    _position: crate::panel::position::PanelPosition,
) -> impl View {
    let mcp_data = window_tab_data.ownstack_mcp.clone();
    let config = window_tab_data.common.config.clone();

    let header = h_stack((
        h_stack((label(|| "MCP SERVERS".to_string()).style(|s| {
            s.font_size(10.0)
                .font_weight(Weight::BOLD)
                .color(Color::from_rgb8(156, 163, 175))
        }),))
        .style(|s| s.items_center()),
        h_stack((
            clickable_icon(
                || LapceIcons::ADD,
                || {},
                || false,
                || false,
                || "Add MCP Server",
                config,
            ),
            clickable_icon(
                || LapceIcons::SETTINGS,
                || {},
                || false,
                || false,
                || "MCP Settings",
                config,
            ),
        ))
        .style(|s| s.items_center().gap(2.0)),
    ))
    .style(|s| {
        s.width_full()
            .justify_between()
            .items_center()
            .padding_horiz(14.0)
            .padding_vert(8.0)
            .background(Color::from_rgb8(17, 17, 27))
            .border_bottom(1.0)
            .border_color(Color::from_rgba8(51, 65, 85, 100))
    });

    // "No servers configured" banner (visible only when empty)
    let no_msg_data = mcp_data.no_config_message;
    let empty_notice = label(move || {
        no_msg_data
            .get()
            .unwrap_or_default()
    })
    .style(move |s| {
        let visible = no_msg_data.get().is_some();
        s.display(if visible {
            floem::style::Display::Flex
        } else {
            floem::style::Display::None
        })
        .padding_horiz(14.0)
        .padding_vert(12.0)
        .font_size(11.0)
        .color(Color::from_rgb8(100, 116, 139))
    });

    let servers_list = scroll(
        dyn_stack(
            move || mcp_data.servers.get(),
            |server| server.name.clone(),
            move |server| {
                let status_color = server.status.color();
                let status_label = server.status.label().to_string();
                let cmd_display = if server.args.is_empty() {
                    server.command.clone()
                } else {
                    format!("{} {}", server.command, server.args.join(" "))
                };
                let source = server.config_source.clone();

                v_stack((
                    h_stack((
                        h_stack((
                            label(|| "".to_string()).style(move |s| {
                                s.width(8.0)
                                    .height(8.0)
                                    .border_radius(99.0)
                                    .background(status_color)
                                    .margin_right(8.0)
                            }),
                            label(move || server.name.clone()).style(|s| {
                                s.font_size(13.0)
                                    .font_weight(Weight::SEMIBOLD)
                                    .color(Color::from_rgb8(203, 213, 225))
                            }),
                        ))
                        .style(|s| s.items_center()),
                        label(move || status_label.clone()).style(|s| {
                            s.color(Color::from_rgb8(100, 116, 139))
                                .font_size(10.0)
                        }),
                    ))
                    .style(|s| s.width_full().justify_between().items_center()),
                    label(move || cmd_display.clone()).style(|s| {
                        s.font_size(10.0)
                            .color(Color::from_rgb8(100, 116, 139))
                            .margin_top(2.0)
                            .margin_left(16.0)
                    }),
                    label(move || source.clone()).style(|s| {
                        s.font_size(9.0)
                            .color(Color::from_rgb8(71, 85, 105))
                            .margin_top(2.0)
                            .margin_left(16.0)
                    }),
                ))
                .style(|s| {
                    s.width_full()
                        .padding_horiz(14.0)
                        .padding_vert(10.0)
                        .border_bottom(1.0)
                        .border_color(Color::from_rgba8(51, 65, 85, 50))
                        .hover(|s| s.background(Color::from_rgba8(255, 255, 255, 10)))
                        .cursor(CursorStyle::Default)
                })
            },
        )
        .style(|s| s.width_full().flex_col()),
    )
    .style(|s| s.width_full().flex_grow(1.0));

    v_stack((header, empty_notice, servers_list)).style(|s: Style| {
        s.width_full()
            .height_full()
            .background(Color::from_rgb8(13, 13, 18))
    })
}
