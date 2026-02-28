use floem::prelude::*;
use floem::reactive::RwSignal;
use floem::{
    peniko::Color,
    style::{CursorStyle, Display, Style},
    text::Weight,
    views::{dyn_stack, h_stack, label, scroll, text_input, v_stack, Decorators},
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
    Available,
    CommandNotFound,
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
    pub config_source: String,
}

// ─── Config file shapes ──────────────────────────────────────────────────────

#[derive(serde::Deserialize, serde::Serialize)]
struct McpServersFile {
    #[serde(default)]
    servers: Vec<McpServerEntry>,
}

#[derive(serde::Deserialize, serde::Serialize, Clone)]
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
        let config_home = std::env::var("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_default();
                std::path::PathBuf::from(home).join(".config")
            });
        Some(config_home.join("Claude").join("claude_desktop_config.json"))
    }
}

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

fn load_mcp_servers(workspace: Option<&std::path::Path>) -> (Vec<McpServerInfo>, Vec<String>) {
    let mut results: Vec<McpServerInfo> = Vec::new();
    let mut searched: Vec<String> = Vec::new();

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

    if let Some(cd_path) = claude_desktop_config_path() {
        let cd_str = cd_path.display().to_string();
        searched.push(cd_str.clone());
        if let Ok(content) = std::fs::read_to_string(&cd_path) {
            if let Ok(parsed) = serde_json::from_str::<ClaudeDesktopConfig>(&content) {
                let mut servers: Vec<_> = parsed.mcp_servers.into_iter().collect();
                servers.sort_by(|a, b| a.0.cmp(&b.0));
                for (name, server) in servers {
                    if results.iter().any(|r| r.name == name) {
                        continue;
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
    pub no_config_message: RwSignal<Option<String>>,
    /// Searched paths (for display in empty state)
    pub searched_paths: RwSignal<String>,
    /// Whether the "Add Server" form is visible
    pub add_form_visible: RwSignal<bool>,
    /// Form fields for new server
    pub form_name: RwSignal<String>,
    pub form_command: RwSignal<String>,
    pub form_args: RwSignal<String>,
    /// Workspace root (for saving config)
    workspace: Option<std::path::PathBuf>,
}

impl OwnStackMcpData {
    pub fn new(common: Rc<CommonData>, workspace: Option<std::path::PathBuf>) -> Self {
        let cx = common.scope;
        let (loaded_servers, searched) = load_mcp_servers(workspace.as_deref());

        let paths_str = if searched.is_empty() {
            "(no workspace)".to_string()
        } else {
            searched.join(", ")
        };

        let no_config_message = if loaded_servers.is_empty() {
            cx.create_rw_signal(Some(format!(
                "No MCP servers configured. Searched: {}",
                paths_str
            )))
        } else {
            cx.create_rw_signal(None)
        };

        let servers = cx.create_rw_signal(loaded_servers);
        Self {
            common: common.clone(),
            servers,
            no_config_message,
            searched_paths: cx.create_rw_signal(paths_str),
            add_form_visible: cx.create_rw_signal(false),
            form_name: cx.create_rw_signal(String::new()),
            form_command: cx.create_rw_signal(String::new()),
            form_args: cx.create_rw_signal(String::new()),
            workspace,
        }
    }

    /// Show the Add Server form
    pub fn show_add_form(&self) {
        self.form_name.set(String::new());
        self.form_command.set(String::new());
        self.form_args.set(String::new());
        self.add_form_visible.set(true);
    }

    /// Hide the Add Server form
    pub fn hide_add_form(&self) {
        self.add_form_visible.set(false);
    }

    /// Save the new server from form fields
    pub fn save_new_server(&self) {
        let name = self.form_name.get_untracked();
        let command = self.form_command.get_untracked();
        let args_raw = self.form_args.get_untracked();

        if name.trim().is_empty() || command.trim().is_empty() {
            return;
        }

        let args: Vec<String> = if args_raw.trim().is_empty() {
            Vec::new()
        } else {
            args_raw.split_whitespace().map(String::from).collect()
        };

        let status = if command_in_path(command.trim()) {
            McpServerStatus::Available
        } else {
            McpServerStatus::CommandNotFound
        };

        let config_source = self.try_persist_server(&name, &command, &args);

        let server = McpServerInfo {
            name: name.trim().to_string(),
            command: command.trim().to_string(),
            args,
            status,
            config_source,
        };

        self.servers.update(|list| list.push(server));
        self.no_config_message.set(None);
        self.hide_add_form();
    }

    /// Attempt to persist to .ownstack/mcp_servers.json
    fn try_persist_server(&self, name: &str, command: &str, args: &[String]) -> String {
        let config_path = if let Some(ws) = &self.workspace {
            ws.join(".ownstack").join("mcp_servers.json")
        } else {
            // Fallback to home config
            let home = std::env::var("HOME").unwrap_or_default();
            std::path::PathBuf::from(home)
                .join(".ownstack")
                .join("mcp_servers.json")
        };

        let mut file: McpServersFile = std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or(McpServersFile { servers: Vec::new() });

        file.servers.push(McpServerEntry {
            name: name.to_string(),
            command: command.to_string(),
            args: args.to_vec(),
            enabled: true,
        });

        if let Some(parent) = config_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match serde_json::to_string_pretty(&file) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&config_path, &json) {
                    tracing::error!("Failed to persist MCP server config: {e}");
                }
            }
            Err(e) => tracing::error!("Failed to serialize MCP config: {e}"),
        }

        config_path.display().to_string()
    }

    /// Reload servers from disk
    pub fn reload(&self) {
        let (loaded, searched) = load_mcp_servers(self.workspace.as_deref());
        let paths_str = if searched.is_empty() {
            "(no workspace)".to_string()
        } else {
            searched.join(", ")
        };
        self.searched_paths.set(paths_str.clone());
        if loaded.is_empty() {
            self.no_config_message.set(Some(format!(
                "No MCP servers configured. Searched: {paths_str}"
            )));
        } else {
            self.no_config_message.set(None);
        }
        self.servers.set(loaded);
    }
}

// ─── Panel UI ─────────────────────────────────────────────────────────────────

pub fn mcp_panel(
    window_tab_data: Rc<WindowTabData>,
    _position: crate::panel::position::PanelPosition,
) -> impl View {
    let mcp_data = window_tab_data.ownstack_mcp.clone();
    let config = window_tab_data.common.config.clone();

    let mcp_add = mcp_data.clone();
    let mcp_reload = mcp_data.clone();

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
                move || mcp_add.show_add_form(),
                || false,
                || false,
                || "Add MCP Server",
                config,
            ),
            clickable_icon(
                || LapceIcons::SETTINGS,
                move || mcp_reload.reload(),
                || false,
                || false,
                || "Reload Servers",
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

    // ── Empty state (elegant, centered) ──────────────────────────────────
    let mcp_empty = mcp_data.clone();
    let empty_view = {
        let searched = mcp_data.searched_paths;
        let no_msg = mcp_data.no_config_message;
        let mcp_cta = mcp_empty.clone();
        v_stack((
            label(|| "MCP").style(|s| {
                s.font_size(28.0)
                    .color(Color::from_rgb8(74, 130, 200))
                    .margin_bottom(12.0)
                    .font_weight(Weight::BOLD)
            }),
            label(|| "No MCP servers configured").style(|s| {
                s.font_size(14.0)
                    .font_weight(Weight::SEMIBOLD)
                    .color(Color::from_rgb8(180, 200, 230))
                    .margin_bottom(6.0)
            }),
            label(|| "Add a server to connect AI agents to external tools and data sources.").style(|s| {
                s.font_size(11.0)
                    .color(Color::from_rgb8(120, 140, 170))
                    .max_width(260.0)
                    .line_height(1.5)
                    .margin_bottom(12.0)
            }),
            label(|| "Add MCP Server")
                .on_click_stop(move |_| mcp_cta.show_add_form())
                .style(|s| {
                    s.padding_horiz(16.0)
                        .padding_vert(8.0)
                        .background(Color::from_rgb8(30, 45, 70))
                        .border(1.0)
                        .border_color(Color::from_rgb8(60, 90, 140))
                        .border_radius(6.0)
                        .color(Color::from_rgb8(180, 210, 255))
                        .font_size(12.0)
                        .cursor(CursorStyle::Pointer)
                        .hover(|s| {
                            s.background(Color::from_rgb8(40, 60, 90))
                                .border_color(Color::from_rgb8(80, 120, 180))
                        })
                        .margin_bottom(14.0)
                }),
            label(move || {
                format!("Searched: {}", searched.get())
            }).style(|s| {
                s.font_size(9.5)
                    .color(Color::from_rgb8(80, 100, 130))
                    .max_width(280.0)
            }),
        ))
        .style(move |s| {
            let visible = no_msg.get().is_some();
            s.apply_if(!visible, |s| s.hide())
                .width_full()
                .flex_grow(1.0)
                .items_center()
                .justify_center()
                .flex_col()
                .padding(20.0)
        })
    };

    // ── Server list ──────────────────────────────────────────────────────
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

    // ── Add Server form (inline, shown/hidden) ───────────────────────────
    let form_data = mcp_empty.clone();
    let form_save = mcp_empty.clone();
    let form_cancel = mcp_empty.clone();
    let form_visible = mcp_empty.add_form_visible;

    let add_form = v_stack((
        label(|| "New MCP Server").style(|s| {
            s.font_size(12.0)
                .font_weight(Weight::BOLD)
                .color(Color::from_rgb8(180, 200, 230))
                .margin_bottom(10.0)
        }),
        label(|| "Name").style(|s| s.font_size(10.0).color(Color::from_rgb8(140, 160, 190))),
        text_input(form_data.form_name)
            .placeholder("e.g. filesystem-server")
            .style(|s| {
                s.width_full()
                    .padding(8.0)
                    .margin_top(4.0)
                    .margin_bottom(8.0)
                    .border(1.0)
                    .border_radius(4.0)
                    .border_color(Color::from_rgba8(51, 65, 85, 150))
                    .background(Color::from_rgb8(18, 22, 32))
                    .color(Color::WHITE)
                    .font_size(12.0)
            }),
        label(|| "Command").style(|s| s.font_size(10.0).color(Color::from_rgb8(140, 160, 190))),
        text_input(form_data.form_command)
            .placeholder("e.g. npx or /usr/local/bin/mcp-server")
            .style(|s| {
                s.width_full()
                    .padding(8.0)
                    .margin_top(4.0)
                    .margin_bottom(8.0)
                    .border(1.0)
                    .border_radius(4.0)
                    .border_color(Color::from_rgba8(51, 65, 85, 150))
                    .background(Color::from_rgb8(18, 22, 32))
                    .color(Color::WHITE)
                    .font_size(12.0)
            }),
        label(|| "Arguments (space-separated)").style(|s| s.font_size(10.0).color(Color::from_rgb8(140, 160, 190))),
        text_input(form_data.form_args)
            .placeholder("e.g. -y @modelcontextprotocol/server-filesystem /tmp")
            .style(|s| {
                s.width_full()
                    .padding(8.0)
                    .margin_top(4.0)
                    .margin_bottom(12.0)
                    .border(1.0)
                    .border_radius(4.0)
                    .border_color(Color::from_rgba8(51, 65, 85, 150))
                    .background(Color::from_rgb8(18, 22, 32))
                    .color(Color::WHITE)
                    .font_size(12.0)
            }),
        h_stack((
            label(|| "Cancel")
                .on_click_stop(move |_| form_cancel.hide_add_form())
                .style(|s| {
                    s.padding_horiz(14.0)
                        .padding_vert(7.0)
                        .border_radius(4.0)
                        .color(Color::from_rgb8(150, 160, 180))
                        .cursor(CursorStyle::Pointer)
                        .hover(|s| s.color(Color::WHITE))
                }),
            label(|| "Save")
                .on_click_stop(move |_| form_save.save_new_server())
                .style(|s| {
                    s.padding_horiz(14.0)
                        .padding_vert(7.0)
                        .background(Color::from_rgb8(37, 99, 235))
                        .border_radius(4.0)
                        .color(Color::WHITE)
                        .font_size(12.0)
                        .cursor(CursorStyle::Pointer)
                        .hover(|s| s.background(Color::from_rgb8(59, 130, 246)))
                }),
        ))
        .style(|s| s.width_full().justify_end().gap(8.0)),
    ))
    .style(move |s| {
        s.apply_if(!form_visible.get(), |s| s.hide())
            .width_full()
            .padding(14.0)
            .background(Color::from_rgb8(20, 24, 36))
            .border_bottom(1.0)
            .border_color(Color::from_rgba8(51, 65, 85, 100))
    });

    v_stack((header, add_form, empty_view, servers_list)).style(|s: Style| {
        s.width_full()
            .height_full()
            .background(Color::from_rgb8(13, 13, 18))
    })
}
