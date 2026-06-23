use crate::app::clickable_icon;
use crate::config::{color::LapceColor, icon::LapceIcons};
use crate::panel::position::PanelPosition;
use floem::prelude::*;
use floem::reactive::{RwSignal, create_rw_signal};
use floem::{
    View,
    ext_event::create_ext_action,
    peniko::Color,
    style::CursorStyle,
    views::{
        container, dyn_container, dyn_stack, h_stack, label, scroll, stack, text,
        text_input, v_stack,
    },
};
use lapce_rpc::ownstack::{AgentModeState, OwnStackRpc};
use lsp_types::DiagnosticSeverity;
use serde::{Deserialize, Serialize};
use std::rc::Rc;

use crate::command::InternalCommand;
use crate::window_tab::CommonData;
use std::path::PathBuf;

/// A single message in the AI chat
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    pub timestamp: String,
    // Optional diff content for preview
    pub diff_content: Option<String>,
    // Optional metadata for roles and results
    #[serde(default)]
    pub sub_role: Option<String>, // "Worker", "Critic", "Healer"
    #[serde(default)]
    pub tool_result: Option<String>,
    #[serde(default)]
    pub diff_target: Option<String>,
    #[serde(default)]
    pub is_error: bool,
}

#[derive(Clone, Debug)]
struct PatchSuggestion {
    path: String,
    new_content: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ChatRole {
    User,
    Assistant,
    System,
    Alert,
    Tool,
}

use crate::window_tab::WindowTabData;
use floem::text::Weight;
use std::sync::Arc;

/// OwnStack Chat Panel — AI interaction interface
#[derive(Clone)]
pub struct OwnStackChatData {
    /// Current input text
    pub input: RwSignal<String>,
    /// Whether the chat panel is visible
    pub visible: RwSignal<bool>,
    /// Chat history
    pub messages: RwSignal<Vec<ChatMessage>>,
    /// Whether the agent is currently processing
    pub is_loading: RwSignal<bool>,
    /// Current agent mode
    pub agent_mode: RwSignal<AgentMode>,
    /// Content currently being streamed from the agent
    pub streaming_content: RwSignal<String>,
    /// Current mission being executed
    #[allow(clippy::type_complexity)]
    pub current_mission: RwSignal<Option<(String, Vec<(String, String)>)>>,
    /// Context-window usage telemetry.
    pub context_current: RwSignal<u64>,
    pub context_max: RwSignal<u64>,
    /// Whether the agent bridge is connected.
    pub bridge_connected: RwSignal<bool>,
    /// Active hub sub-tab (Chat / Tools / Audit).
    pub hub_tab: RwSignal<OwnStackHubTab>,
    common: CommonData,
    db: Arc<crate::db::LapceDb>,
}

/// Agent execution mode
#[derive(Clone, Debug, PartialEq)]
pub enum AgentMode {
    Ask,
    Auto,
    Plan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChatMonitorTab {
    Output,
    Problems,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnStackHubTab {
    Chat,
    Tools,
    Audit,
}

impl std::fmt::Display for AgentMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentMode::Ask => write!(f, "Ask"),
            AgentMode::Auto => write!(f, "Auto"),
            AgentMode::Plan => write!(f, "Plan"),
        }
    }
}

impl AgentMode {
    pub fn from_preference(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "auto" => Self::Auto,
            "plan" => Self::Plan,
            _ => Self::Ask,
        }
    }

    pub fn from_runtime(mode: AgentModeState) -> Self {
        match mode {
            AgentModeState::Ask => Self::Ask,
            AgentModeState::Auto => Self::Auto,
            AgentModeState::Plan => Self::Plan,
        }
    }

    pub fn to_runtime(&self) -> AgentModeState {
        match self {
            AgentMode::Ask => AgentModeState::Ask,
            AgentMode::Auto => AgentModeState::Auto,
            AgentMode::Plan => AgentModeState::Plan,
        }
    }

    /// Returns the accent color for this mode.
    pub fn color(&self) -> Color {
        match self {
            AgentMode::Ask => crate::ownstack_theme::ACCENT,
            AgentMode::Auto => crate::ownstack_theme::MODE_AUTO,
            AgentMode::Plan => crate::ownstack_theme::MODE_PLAN,
        }
    }

    /// Returns the label with an emoji indicator.
    pub fn label_with_icon(&self) -> &'static str {
        match self {
            AgentMode::Ask => "💬 Ask",
            AgentMode::Auto => "⚡ Auto",
            AgentMode::Plan => "🗺 Plan",
        }
    }
}

impl OwnStackChatData {
    pub fn new(common: CommonData, db: Arc<crate::db::LapceDb>) -> Self {
        let workspace = common.workspace.clone();
        let messages = db
            .get_ownstack_chat(&workspace)
            .unwrap_or_else(|_| Vec::new());

        Self {
            input: create_rw_signal(String::new()),
            visible: create_rw_signal(false),
            messages: create_rw_signal(messages),
            is_loading: create_rw_signal(false),
            agent_mode: create_rw_signal(AgentMode::Ask),
            streaming_content: create_rw_signal(String::new()),
            current_mission: create_rw_signal(None),
            context_current: create_rw_signal(0),
            context_max: create_rw_signal(0),
            bridge_connected: create_rw_signal(false),
            hub_tab: create_rw_signal(OwnStackHubTab::Chat),
            common,
            db,
        }
    }

    /// Toggle chat panel visibility
    pub fn toggle(&self) {
        let current = self.visible.get_untracked();
        self.visible.set(!current);
    }

    /// Show the chat panel
    pub fn show(&self) {
        self.visible.set(true);
    }

    /// Hide the chat panel
    pub fn hide(&self) {
        self.visible.set(false);
    }
    /// Send a message from the user
    pub fn send_message(&self) {
        let prompt = self.input.get_untracked();
        if prompt.trim().is_empty() {
            return;
        }

        // Guard: no AI provider configured (e.g. onboarding was skipped without
        // a key). Show a clear, actionable message instead of firing a request
        // that would fail with an API-key error.
        if !crate::ownstack_onboarding::is_ai_provider_configured() {
            self.messages.update(|msgs| {
                msgs.push(ChatMessage {
                    role: ChatRole::Alert,
                    content: "No AI provider is configured. Open Settings to add an API key (OpenRouter, Anthropic, or a custom OpenAI-compatible endpoint), then try again.".to_string(),
                    timestamp: chrono_now(),
                    diff_content: None,
                    sub_role: None,
                    tool_result: None,
                    diff_target: None,
                    is_error: true,
                });
            });
            return;
        }

        // Guard: warn if bridge is disconnected
        if !self.bridge_connected.get_untracked() {
            self.messages.update(|msgs| {
                msgs.push(ChatMessage {
                    role: ChatRole::Alert,
                    content: "Agent bridge is disconnected. Your message cannot be delivered. Check the status bar for connection status.".to_string(),
                    timestamp: chrono_now(),
                    diff_content: None,
                    sub_role: None,
                    tool_result: None,
                    diff_target: None,
                    is_error: true,
                });
            });
            return;
        }

        // Add user message to history
        let user_msg = ChatMessage {
            role: ChatRole::User,
            content: prompt.clone(),
            timestamp: chrono_now(),
            diff_content: None,
            sub_role: None,
            tool_result: None,
            diff_target: None,
            is_error: false,
        };

        self.messages.update(|msgs| {
            msgs.push(user_msg);
        });

        // Auto-save
        self.db.save_ownstack_chat(
            self.common.workspace.clone(),
            self.messages.get_untracked(),
        );

        // Clear input
        self.input.set(String::new());

        // Set loading state
        self.is_loading.set(true);
        self.streaming_content.set(String::new());

        // Send via RPC
        let message = OwnStackRpc::AiPrompt { prompt };
        self.common.proxy.ownstack(message);
        tracing::info!("OwnStack Chat: AiPrompt sent");
    }

    /// Receive a response from the AI
    pub fn receive_response(&self, content: String, diff: Option<String>) {
        let timestamp = chrono_now();
        let assistant_msg = ChatMessage {
            role: ChatRole::Assistant,
            content: content.clone(),
            timestamp: timestamp.clone(),
            diff_content: diff.clone(),
            sub_role: None,
            tool_result: None,
            diff_target: None,
            is_error: false,
        };
        self.messages.update(|msgs| {
            msgs.push(assistant_msg);
        });

        // Auto-save
        self.db.save_ownstack_chat(
            self.common.workspace.clone(),
            self.messages.get_untracked(),
        );

        if diff.is_none() {
            // If no diff provided (common during streaming end), trigger background computation
            self.trigger_background_diff(content, timestamp);
        }

        self.is_loading.set(false);
        self.streaming_content.set(String::new());
    }

    /// Receive a stream chunk from the AI
    pub fn receive_chunk(&self, chunk: OwnStackRpc) {
        if let OwnStackRpc::AiStreamChunk {
            content_delta,
            tool_call_delta: _,
            finish_reason,
        } = chunk
        {
            if let Some(delta) = content_delta {
                self.streaming_content.update(|c| c.push_str(&delta));
            }
            if let Some(_reason) = finish_reason {
                let content = self.streaming_content.get_untracked();
                if !content.is_empty() {
                    // DO NOT call derive_diff_preview synchronously here.
                    // It involves disk I/O and expensive LCS diffing.
                    // We call receive_response with None for diff, and it triggers background diffing.
                    self.receive_response(content, None);
                }
            }
        }
    }

    fn trigger_background_diff(&self, content: String, timestamp: String) {
        let workspace_root = self.common.workspace.path.clone();
        let db = self.db.clone();
        let messages = self.messages;
        let workspace = self.common.workspace.clone();
        let scope = self.common.scope;

        let send = create_ext_action(scope, move |diff: Option<String>| {
            if let Some(diff) = diff {
                messages.update(|msgs| {
                    if let Some(msg) = msgs.iter_mut().rev().find(|m| {
                        m.timestamp == timestamp && m.role == ChatRole::Assistant
                    }) {
                        msg.diff_content = Some(diff);
                    }
                });

                // Save after updating diff
                db.save_ownstack_chat(workspace, messages.get_untracked());
            }
        });

        std::thread::spawn(move || {
            // Expensive operations moved to background thread
            let diff = if let Some(patch) = extract_structured_patch(&content) {
                if let Some(workspace_path) = workspace_root {
                    let path = std::path::Path::new(&patch.path);
                    if is_safe_workspace_relative_path(path) {
                        let full_path = workspace_path.join(path);
                        // Blocking I/O
                        let old_content =
                            std::fs::read_to_string(&full_path).unwrap_or_default();
                        // Expensive LCS
                        Some(render_unified_diff(
                            &patch.path,
                            &old_content,
                            &patch.new_content,
                        ))
                    } else {
                        Some(format!(
                            "Rejected patch path outside workspace: {}",
                            patch.path
                        ))
                    }
                } else {
                    None
                }
            } else {
                extract_code_block(&content)
            };

            send(diff);
        });
    }

    /// Receive a mission update
    pub fn receive_mission(&self, goal: String, steps: Vec<(String, String)>) {
        self.current_mission.set(Some((goal, steps)));
    }

    /// Send a decision on a suggestion
    pub fn send_decision(&self, decision: &str, message_id: &str) {
        let rpc = OwnStackRpc::SuggestionDecision {
            decision: decision.to_string(),
            message_id: message_id.to_string(),
        };
        self.common.proxy.ownstack(rpc);
    }

    /// Update context-window usage telemetry for UI rendering.
    pub fn set_context_window(&self, current: u64, max: u64) {
        self.context_current.set(current);
        self.context_max.set(max);
    }

    /// Apply runtime mode received from the agent.
    pub fn set_mode_from_runtime(&self, mode: AgentModeState) {
        self.agent_mode.set(AgentMode::from_runtime(mode));
    }

    /// Stop the current operation
    pub fn stop(&self) {
        // Kill-switch is enforced in the proxy (owns the agent process handle).
        self.common.proxy.ownstack(OwnStackRpc::KillSwitch);
        self.is_loading.set(false);
        let content = self.streaming_content.get_untracked();
        if !content.is_empty() {
            self.receive_response(content, None);
        }
        self.streaming_content.set(String::new());
        self.add_alert_message(
            "Kill-switch activated. Current agent run stopped.".to_string(),
        );
    }

    /// Add a system message
    pub fn add_system_message(&self, content: String) {
        let sys_msg = ChatMessage {
            role: ChatRole::System,
            content,
            timestamp: chrono_now(),
            diff_content: None,
            sub_role: None,
            tool_result: None,
            diff_target: None,
            is_error: false,
        };

        self.messages.update(|msgs| {
            msgs.push(sys_msg);
        });
    }

    pub fn add_alert_message(&self, content: String) {
        let alert_msg = ChatMessage {
            role: ChatRole::Alert,
            content,
            timestamp: chrono_now(),
            diff_content: None,
            sub_role: None,
            tool_result: None,
            diff_target: None,
            is_error: true,
        };

        self.messages.update(|msgs| {
            msgs.push(alert_msg);
        });
    }

    /// Clear chat history
    pub fn clear_history(&self) {
        self.messages.set(Vec::new());
        self.db.save_ownstack_chat(
            self.common.workspace.clone(),
            self.messages.get_untracked(),
        );
    }

    /// Set a specific agent mode.
    pub fn set_mode(&self, mode: AgentMode) {
        tracing::info!("Agent mode set: {:?}", mode);
        self.common.proxy.ownstack(OwnStackRpc::SetAgentMode {
            mode: mode.to_runtime(),
        });
    }

    /// Cycle through agent modes: Ask → Auto → Plan → Ask
    pub fn cycle_mode(&self) {
        let current = self.agent_mode.get_untracked();
        let next = match current {
            AgentMode::Ask => AgentMode::Auto,
            AgentMode::Auto => AgentMode::Plan,
            AgentMode::Plan => AgentMode::Ask,
        };
        self.set_mode(next);
    }

    /// Get message count
    pub fn message_count(&self) -> usize {
        self.messages.get_untracked().len()
    }
}

pub fn ownstack_chat_panel(
    window_tab_data: Rc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    let chat_data = window_tab_data.ownstack_chat.clone();
    let hub_tab = chat_data.hub_tab;
    let config_hub = window_tab_data.common.config;

    // ── Hub tab bar ──────────────────────────────────────────────────────
    fn hub_tab_segment(
        hub_tab: RwSignal<OwnStackHubTab>,
        tab: OwnStackHubTab,
    ) -> impl View {
        let lbl = match tab {
            OwnStackHubTab::Chat  => "Chat",
            OwnStackHubTab::Tools => "Tools",
            OwnStackHubTab::Audit => "Audit",
        };
        label(move || lbl)
            .style(move |s| {
                let active = hub_tab.get() == tab;
                let base = s
                    .padding_horiz(12.0)
                    .padding_vert(5.0)
                    .font_size(11.0)
                    .font_weight(Weight::BOLD)
                    .cursor(CursorStyle::Pointer)
                    .border_radius(4.0);
                if active {
                    base.background(crate::ownstack_theme::ACCENT.multiply_alpha(0.20))
                        .color(crate::ownstack_theme::ACCENT_BRIGHT)
                } else {
                    base.color(crate::ownstack_theme::TEXT_DIM)
                        .hover(|s| s.background(crate::ownstack_theme::SURFACE_HOVER))
                }
            })
            .on_click_stop(move |_| hub_tab.set(tab))
    }

    let hub_bar = h_stack((
        hub_tab_segment(hub_tab, OwnStackHubTab::Chat),
        hub_tab_segment(hub_tab, OwnStackHubTab::Tools),
        hub_tab_segment(hub_tab, OwnStackHubTab::Audit),
    ))
    .style(move |s| {
        let config = config_hub.get();
        s.width_full()
            .items_center()
            .gap(2.0)
            .padding_horiz(10.0)
            .padding_vert(6.0)
            .border_bottom(1.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
    });

    // ── Chat sub-view (built on demand by the hub switcher) ──────────────
    fn ownstack_chat_view(window_tab_data: Rc<WindowTabData>) -> impl View {
    let chat_data = window_tab_data.ownstack_chat.clone();
    let config = window_tab_data.common.config;
    let input = chat_data.input;
    let diagnostics = window_tab_data.main_split.diagnostics;
    let workspace_root = window_tab_data.workspace.path.clone();
    let monitor_tab = create_rw_signal(ChatMonitorTab::Output);

    let chat_data_trash = chat_data.clone();
    let chat_data_attach = chat_data.clone();
    let chat_data_output = chat_data.clone();
    let monitor_tab_output = monitor_tab;
    let monitor_tab_problems = monitor_tab;
    let monitor_tab_list = monitor_tab;

    v_stack((
        // ── Header ───────────────────────────────────────────────────────
        h_stack((
            // Title
            text("OwnStack AI")
                .style(|s| s.font_weight(Weight::BOLD).font_size(13.0)),
            // Right-side controls
            h_stack((
                // ① Segmented mode selector: Ask | Plan | Auto
                {
                    fn mode_segment(chat: OwnStackChatData, mode: AgentMode) -> impl View {
                        let lbl = match mode {
                            AgentMode::Ask  => "Ask",
                            AgentMode::Plan => "Plan",
                            AgentMode::Auto => "Auto",
                        };
                        let mc = mode.color();
                        let target = mode.clone();
                        label(move || lbl)
                            .style(move |s| {
                                let active = chat.agent_mode.get() == target;
                                let base = s
                                    .padding_horiz(10.0)
                                    .padding_vert(3.0)
                                    .font_size(11.0)
                                    .font_weight(Weight::BOLD)
                                    .cursor(CursorStyle::Pointer)
                                    .border_radius(4.0);
                                if active {
                                    base.background(mc.multiply_alpha(0.25)).color(mc)
                                } else {
                                    base.color(crate::ownstack_theme::TEXT_DIM)
                                        .hover(|s| s.background(crate::ownstack_theme::SURFACE_HOVER))
                                }
                            })
                            .on_click_stop({
                                let chat = chat.clone();
                                let m = mode.clone();
                                move |_| chat.set_mode(m.clone())
                            })
                    }
                    h_stack((
                        mode_segment(chat_data.clone(), AgentMode::Ask),
                        mode_segment(chat_data.clone(), AgentMode::Plan),
                        mode_segment(chat_data.clone(), AgentMode::Auto),
                    ))
                    .style(|s| {
                        s.items_center()
                            .gap(2.0)
                            .padding(2.0)
                            .border(1.0)
                            .border_radius(6.0)
                            .border_color(crate::ownstack_theme::BORDER)
                            .background(crate::ownstack_theme::SURFACE_1)
                    })
                },
                // ② Trash — Clear History
                clickable_icon(
                    || LapceIcons::TRASH,
                    {
                        let chat_data = chat_data_trash.clone();
                        move || chat_data.clear_history()
                    },
                    || false,
                    || false,
                    || "Clear History",
                    config,
                ),
            ))
            .style(|s| s.items_center().gap(8.0)),
        ))
        .style(move |s| {
            let config = config.get();
            s.width_full()
                .padding(10.0)
                .justify_between()
                .items_center()
                .border_bottom(1.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER))
                .background(
                    config
                        .color(LapceColor::PANEL_BACKGROUND)
                        .multiply_alpha(0.8),
                )
        }),
        // Messages list (with empty state when no messages)
        scroll(
            v_stack((
                // Empty state — shown when there are no messages yet
                crate::ownstack_empty_state::chat_empty_state()
                    .style(move |s| {
                        let has_messages = !chat_data.messages.get().is_empty()
                            || chat_data.current_mission.get().is_some()
                            || !chat_data.streaming_content.get().is_empty();
                        s.apply_if(has_messages, |s| s.hide())
                    })
                    .into_any(),
                // Current Mission Display (if active)
                dyn_stack(
                    move || {
                        if let Some((goal, steps)) = chat_data.current_mission.get()
                        {
                            vec![(goal, steps)]
                        } else {
                            vec![]
                        }
                    },
                    |(goal, _)| goal.clone(),
                    move |(goal, steps)| {
                        v_stack((
                            label(move || format!("Mission: {}", goal)).style(|s| {
                                s.font_weight(Weight::BOLD)
                                    .padding_bottom(5.0)
                                    .color(crate::ownstack_theme::TEXT)
                            }),
                            v_stack((dyn_stack(
                                move || steps.clone(),
                                |(desc, _)| desc.clone(),
                                move |(desc, status)| {
                                    let status_for_icon = status.clone();
                                    let status_for_color = status.clone();
                                    h_stack((
                                        label(move || {
                                            match status_for_icon
                                                .to_ascii_lowercase()
                                                .as_str()
                                            {
                                                "pending" => "○",
                                                "inprogress" => "⟳",
                                                "completed" | "done" => "✓",
                                                "failed" => "✕",
                                                _ => "•",
                                            }
                                        })
                                        .style(move |s| {
                                            let color = match status_for_color
                                                .to_ascii_lowercase()
                                                .as_str()
                                            {
                                                "inprogress" => {
                                                    crate::ownstack_theme::STEP_ACTIVE
                                                }
                                                "completed" | "done" => {
                                                    crate::ownstack_theme::STEP_DONE
                                                }
                                                "failed" => {
                                                    crate::ownstack_theme::STEP_FAILED
                                                }
                                                _ => crate::ownstack_theme::STEP_PENDING,
                                            };
                                            s.width(20.0).color(color)
                                        }),
                                        label(move || desc.clone()),
                                    ))
                                },
                            ),)),
                        ))
                        .style(move |s| {
                            let config = config.get();
                            s.width_full()
                                .padding(10.0)
                                .margin_bottom(10.0)
                                .border(1.0)
                                .border_radius(6.0)
                                .border_color(config.color(LapceColor::LAPCE_BORDER))
                                .background(
                                    config
                                        .color(LapceColor::PANEL_BACKGROUND)
                                        .multiply_alpha(0.6),
                                )
                                .box_shadow_blur(10.0)
                                .box_shadow_color(crate::ownstack_theme::ACCENT.multiply_alpha(0.12))
                        })
                    },
                ),
                dyn_stack(
                    move || chat_data.messages.get(),
                    |msg| (msg.timestamp.clone(), msg.content.clone()),
                    {
                        let chat_data = chat_data.clone();
                        move |msg| message_view(msg, chat_data.clone(), config)
                    },
                ),
                // Streaming message
                dyn_stack(
                    {
                        let chat_data = chat_data.clone();
                        move || {
                            let content = chat_data.streaming_content.get();
                            if content.is_empty() {
                                vec![]
                            } else {
                                vec![content]
                            }
                        }
                    },
                    |content| content.clone(),
                    {
                        let chat_data = chat_data.clone();
                        move |content| {
                            message_view(
                                ChatMessage {
                                    role: ChatRole::Assistant,
                                    content,
                                    timestamp: chrono_now(),
                                    diff_content: None,
                                    sub_role: None,
                                    tool_result: None,
                                    diff_target: None,
                                    is_error: false,
                                },
                                chat_data.clone(),
                                config,
                            )
                        }
                    },
                ),
                // "Thinking" indicator — visible before first streaming token arrives
                {
                    let chat_data = chat_data.clone();
                    label(move || "AI is thinking\u{2026}")
                        .style(move |s| {
                            let visible = chat_data.is_loading.get()
                                && chat_data.streaming_content.get().is_empty();
                            s.apply_if(!visible, |s| s.hide())
                                .padding(14.0)
                                .border_radius(12.0)
                                .background(crate::ownstack_theme::SURFACE_0)
                                .border(1.0)
                                .border_color(crate::ownstack_theme::BORDER)
                                .color(crate::ownstack_theme::TEXT_DIM)
                                .font_size(12.0)
                                .font_style(floem::text::Style::Italic)
                        })
                },
            ))
            .style(|s| s.width_full().padding(12.0).gap(20.0)),
        )
        .style(|s| s.flex_grow(1.0).width_full()),
        container(
            v_stack((
                h_stack((
                    label(|| "Output".to_string())
                        .on_click_stop(move |_| {
                            monitor_tab_output.set(ChatMonitorTab::Output);
                        })
                        .style(move |s| {
                            let is_active =
                                monitor_tab_output.get() == ChatMonitorTab::Output;
                            s.padding_horiz(10.0)
                                .padding_vert(5.0)
                                .border(1.0)
                                .border_radius(6.0)
                                .border_color(
                                    config.get().color(LapceColor::LAPCE_BORDER),
                                )
                                .background(if is_active {
                                    crate::ownstack_theme::ACCENT.multiply_alpha(0.16)
                                } else {
                                    Color::TRANSPARENT
                                })
                                .color(if is_active {
                                    crate::ownstack_theme::ACCENT_BRIGHT
                                } else {
                                    config.get().color(LapceColor::STATUS_FOREGROUND)
                                })
                                .font_size(11.0)
                                .font_weight(if is_active { Weight::BOLD } else { Weight::NORMAL })
                                .cursor(CursorStyle::Pointer)
                                .hover(|s| s.background(crate::ownstack_theme::SURFACE_HOVER))
                        }),
                    label(|| "Problems".to_string())
                        .on_click_stop(move |_| {
                            monitor_tab_problems.set(ChatMonitorTab::Problems);
                        })
                        .style(move |s| {
                            let is_active = monitor_tab_problems.get()
                                == ChatMonitorTab::Problems;
                            s.padding_horiz(10.0)
                                .padding_vert(5.0)
                                .border(1.0)
                                .border_radius(6.0)
                                .border_color(
                                    config.get().color(LapceColor::LAPCE_BORDER),
                                )
                                .background(if is_active {
                                    crate::ownstack_theme::STATE_WARN.multiply_alpha(0.16)
                                } else {
                                    Color::TRANSPARENT
                                })
                                .color(if is_active {
                                    crate::ownstack_theme::STATE_WARN
                                } else {
                                    config.get().color(LapceColor::STATUS_FOREGROUND)
                                })
                                .font_size(11.0)
                                .font_weight(if is_active { Weight::BOLD } else { Weight::NORMAL })
                                .cursor(CursorStyle::Pointer)
                                .hover(|s| s.background(crate::ownstack_theme::SURFACE_HOVER))
                        }),
                ))
                .style(|s| s.width_full().items_center().gap(8.0)),
                scroll(dyn_stack(
                    move || {
                        if monitor_tab_list.get() == ChatMonitorTab::Output {
                            let mut lines = chat_data_output
                                .messages
                                .get()
                                .into_iter()
                                .rev()
                                .filter(|msg| {
                                    matches!(
                                        msg.role,
                                        ChatRole::System
                                            | ChatRole::Tool
                                            | ChatRole::Alert
                                    )
                                })
                                .map(|msg| summarize_output_entry(&msg))
                                .take(20)
                                .collect::<Vec<_>>();
                            if lines.is_empty() {
                                lines.push(
                                    "No recent system/tool output.".to_string(),
                                );
                            }
                            lines
                        } else {
                            let mut lines = Vec::new();
                            let diagnostics_map = diagnostics.get();
                            for (path, diagnostic_data) in diagnostics_map.iter() {
                                let display_path = format_problem_path(
                                    path,
                                    workspace_root.as_deref(),
                                );
                                for diagnostic in
                                    diagnostic_data.diagnostics.get().iter()
                                {
                                    let line = diagnostic.range.start.line + 1;
                                    let severity =
                                        severity_label(diagnostic.severity);
                                    let summary =
                                        first_line(&diagnostic.message, 180);
                                    lines.push(format!(
                                        "{severity} {display_path}:{line} {summary}"
                                    ));
                                    if lines.len() >= 20 {
                                        break;
                                    }
                                }
                                if lines.len() >= 20 {
                                    break;
                                }
                            }
                            if lines.is_empty() {
                                lines.push("No active diagnostics.".to_string());
                            }
                            lines
                        }
                    },
                    |line| line.clone(),
                    move |line| {
                        label(move || line.clone()).style(move |s| {
                            let config = config.get();
                            s.width_full()
                                .font_size(10.5)
                                .line_height(1.35)
                                .font_family("monospace".to_string())
                                .color(config.color(LapceColor::EDITOR_DIM))
                                .padding_bottom(4.0)
                        })
                    },
                ))
                .style(|s| s.width_full().max_height(96.0).padding_top(6.0)),
            ))
            .style(move |s| {
                let config = config.get();
                s.width_full()
                    .padding_horiz(10.0)
                    .padding_top(8.0)
                    .padding_bottom(6.0)
                    .border_top(1.0)
                    .border_color(config.color(LapceColor::LAPCE_BORDER))
                    .background(
                        config
                            .color(LapceColor::PANEL_BACKGROUND)
                            .multiply_alpha(0.4),
                    )
            }),
        )
        .style(|s| s.width_full()),
        // Input area
        container(
            v_stack((
                {
                    let context_label = chat_data.clone();
                    let context_fill = chat_data.clone();
                    v_stack((
                        label(move || {
                            let current = context_label.context_current.get();
                            let max = context_label.context_max.get();
                            if max == 0 {
                                "Context: waiting for connection".to_string()
                            } else {
                                format!("Context: {current}/{max} tokens used")
                            }
                        })
                        .style(move |s| {
                            let config = config.get();
                            s.font_size(10.0)
                                .color(config.color(LapceColor::EDITOR_DIM))
                                .padding_bottom(4.0)
                        }),
                        stack((
                            label(String::new).style(move |s| {
                                let config = config.get();
                                s.width_full()
                                    .height(4.0)
                                    .border_radius(999.0)
                                    .background(
                                        config
                                            .color(
                                                LapceColor::PANEL_HOVERED_BACKGROUND,
                                            )
                                            .multiply_alpha(0.9),
                                    )
                            }),
                            label(String::new).style(move |s| {
                                let current = context_fill.context_current.get();
                                let max = context_fill.context_max.get();
                                let ratio = if max == 0 {
                                    0.0
                                } else {
                                    ((current as f64 / max as f64) * 100.0)
                                        .clamp(0.0, 100.0)
                                };
                                let fill = if ratio >= 95.0 {
                                    crate::ownstack_theme::STATE_ERROR
                                } else if ratio >= 80.0 {
                                    crate::ownstack_theme::STATE_WARN
                                } else {
                                    crate::ownstack_theme::STATE_OK
                                };
                                s.width_pct(ratio)
                                    .height(4.0)
                                    .border_radius(999.0)
                                    .background(fill)
                            }),
                        ))
                        .style(|s| s.width_full()),
                    ))
                    .style(|s| s.width_full().padding_bottom(8.0))
                },
                // ── Bridge disconnected warning ──────────────────────────
                // The proxy reconnects automatically, so this banner is purely
                // informational and disappears on its own once the bridge is
                // back. No action button (it would be a dead affordance).
                {
                    let chat_data_dc = chat_data.clone();
                    h_stack((
                        label(|| "\u{26A0}").style(|s| {
                            s.margin_right(6.0)
                                .font_size(11.0)
                                .color(crate::ownstack_theme::STATE_WARN)
                        }),
                        label(|| "Agent bridge disconnected — reconnecting automatically\u{2026}")
                            .style(|s| s.flex_grow(1.0).font_size(11.0).color(crate::ownstack_theme::STATE_WARN)),
                    ))
                    .style(move |s| {
                        let connected = chat_data_dc.bridge_connected.get();
                        s.width_full()
                            .items_center()
                            .padding_horiz(10.0)
                            .padding_vert(6.0)
                            .background(crate::ownstack_theme::STATE_WARN.multiply_alpha(0.08))
                            .border_radius(6.0)
                            .margin_bottom(6.0)
                            .apply_if(connected, |s| s.hide())
                    })
                },
                h_stack((
                    label(|| "+ Context")
                        .style(move |s| {
                            let config = config.get();
                            s.padding(6.0)
                                .border_radius(6.0)
                                .font_size(12.0)
                                .cursor(floem::style::CursorStyle::Pointer)
                                .color(config.color(LapceColor::EDITOR_DIM))
                                .hover(|s| {
                                    s.background(crate::ownstack_theme::ACCENT.multiply_alpha(0.16))
                                    .color(crate::ownstack_theme::ACCENT)
                                })
                        })
                        .on_click_stop(move |_| {
                            chat_data_attach
                                .common
                                .proxy
                                .ownstack(OwnStackRpc::UiSnapshotRequest);
                            chat_data_attach.add_system_message(
                                "UI context snapshot requested".to_string(),
                            );
                        }),
                    text_input(input)
                        .placeholder("Type a message to OwnStack...")
                        .style(move |s| {
                            let config = config.get();
                            s.width_full()
                                .padding(8.0)
                                .border(1.0)
                                .border_radius(10.0)
                                .border_color(config.color(LapceColor::LAPCE_BORDER))
                                .background(
                                    config.color(LapceColor::EDITOR_BACKGROUND),
                                )
                                .hover(|s| {
                                    s.border_color(crate::ownstack_theme::ACCENT)
                                })
                                .active(|s| {
                                    s.border_color(crate::ownstack_theme::ACCENT_BRIGHT)
                                        .box_shadow_blur(5.0)
                                        .box_shadow_color(crate::ownstack_theme::ACCENT.multiply_alpha(0.39))
                                })
                        })
                        .on_event_stop(floem::event::EventListener::KeyDown, {
                            let chat_data = chat_data.clone();
                            move |event| {
                                if let floem::event::Event::KeyDown(ke) = event {
                                    if ke.key.logical_key
                                        == floem::keyboard::Key::Named(
                                            floem::keyboard::NamedKey::Enter,
                                        )
                                    {
                                        chat_data.send_message();
                                    }
                                }
                            }
                        }),
                    stack((
                        clickable_icon(
                            || LapceIcons::SEND,
                            {
                                let chat_data = chat_data.clone();
                                move || {
                                    chat_data.send_message();
                                }
                            },
                            {
                                let chat_data = chat_data.clone();
                                move || chat_data.is_loading.get()
                            },
                            || false,
                            || "Send",
                            config,
                        )
                        .style({
                            let chat_data = chat_data.clone();
                            move |s| {
                                s.apply_if(chat_data.is_loading.get(), |s| s.hide())
                            }
                        }),
                        clickable_icon(
                            || LapceIcons::STOP,
                            {
                                let chat_data = chat_data.clone();
                                move || {
                                    chat_data.stop();
                                }
                            },
                            || false,
                            || false,
                            || "Stop",
                            config,
                        )
                        .style({
                            let chat_data = chat_data.clone();
                            move |s| {
                                s.apply_if(!chat_data.is_loading.get(), |s| s.hide())
                            }
                        }),
                    )),
                ))
                .style(|s| s.width_full().items_center().gap(10.0)),
            ))
            .style(|s| s.width_full()),
        )
        .style(move |s| {
            let config = config.get();
            s.width_full()
                .padding(10.0)
                .border_top(1.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER))
        }),
        // ── Shortcut hints bar ──────────────────────────────────────
        h_stack((
            shortcut_hint("Enter", "Send"),
            shortcut_hint("Cmd/Ctrl+K", "Inline Edit"),
            shortcut_hint("Cmd/Ctrl+L", "Toggle Chat"),
            shortcut_hint("Cmd/Ctrl+Shift+P", "AI Palette"),
        ))
        .style(move |s| {
            let config = config.get();
            s.width_full()
                .padding_horiz(10.0)
                .padding_vert(4.0)
                .gap(12.0)
                .justify_center()
                .border_top(1.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER).multiply_alpha(0.5))
        }),
    ))
    .style(|s| s.size_full().flex_col())
    } // end ownstack_chat_view

    // ── Compose hub: only the active sub-view is mounted ─────────────────
    // Using dyn_container (rather than hiding all three) avoids wasted
    // reactivity in the inactive panels and keeps hidden inputs out of the
    // focus/tab order.
    let wtd = window_tab_data.clone();
    let content = dyn_container(
        move || hub_tab.get(),
        move |tab| match tab {
            OwnStackHubTab::Chat => {
                ownstack_chat_view(wtd.clone()).into_any()
            }
            OwnStackHubTab::Tools => {
                container(crate::ownstack_mcp::mcp_panel(wtd.clone(), position))
                    .style(|s| s.size_full())
                    .into_any()
            }
            OwnStackHubTab::Audit => {
                container(crate::ownstack_audit::audit_panel(
                    wtd.clone(),
                    position,
                ))
                .style(|s| s.size_full())
                .into_any()
            }
        },
    )
    .style(|s| s.size_full());

    v_stack((hub_bar, content)).style(|s| s.size_full().flex_col())
}

fn summarize_output_entry(msg: &ChatMessage) -> String {
    let role = match msg.role {
        ChatRole::System => "system",
        ChatRole::Tool => "tool",
        ChatRole::Alert => "alert",
        ChatRole::User => "user",
        ChatRole::Assistant => "assistant",
    };
    let body = first_line(&msg.content, 180);
    format!("[{}] {role}: {body}", msg.timestamp)
}

fn first_line(content: &str, max_chars: usize) -> String {
    let line = content
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .replace('\t', " ");
    if line.chars().count() <= max_chars {
        line
    } else {
        let mut out = String::new();
        for c in line.chars().take(max_chars.saturating_sub(1)) {
            out.push(c);
        }
        out.push_str("...");
        out
    }
}

fn severity_label(severity: Option<DiagnosticSeverity>) -> &'static str {
    match severity {
        Some(DiagnosticSeverity::ERROR) => "error",
        Some(DiagnosticSeverity::WARNING) => "warn",
        Some(DiagnosticSeverity::INFORMATION) => "info",
        Some(DiagnosticSeverity::HINT) => "hint",
        _ => "diag",
    }
}

fn format_problem_path(
    path: &std::path::Path,
    workspace_root: Option<&std::path::Path>,
) -> String {
    if let Some(root) = workspace_root {
        if let Ok(rel) = path.strip_prefix(root) {
            return rel.display().to_string();
        }
    }
    path.display().to_string()
}

fn message_view(
    msg: ChatMessage,
    chat_data: OwnStackChatData,
    config: floem::reactive::ReadSignal<Arc<crate::config::LapceConfig>>,
) -> impl View {
    let is_user = msg.role == ChatRole::User;

    let bubble_bg = if is_user {
        crate::ownstack_theme::ACCENT_DIM
    } else if msg.is_error {
        crate::ownstack_theme::STATE_ERROR.multiply_alpha(0.31)
    } else {
        crate::ownstack_theme::SURFACE_0
    };

    let bubble_border = if is_user {
        Color::TRANSPARENT
    } else if msg.is_error {
        crate::ownstack_theme::STATE_ERROR.multiply_alpha(0.47)
    } else {
        crate::ownstack_theme::SURFACE_0.multiply_alpha(0.59)
    };

    let text_color = if is_user {
        Color::WHITE
    } else {
        crate::ownstack_theme::TEXT
    };

    let role_dot_color = if msg.is_error {
        crate::ownstack_theme::STATE_WARN
    } else {
        match msg.sub_role.as_deref() {
            Some("Critic") => crate::ownstack_theme::STATE_ERROR,
            Some("System") => crate::ownstack_theme::TEXT_DIM,
            _ => crate::ownstack_theme::ACCENT,
        }
    };

    let role_label_text = if msg.is_error {
        if msg.content.contains("KILL-SWITCH") {
            "SYSTEM ALERT".to_string()
        } else {
            "HEALER".to_string()
        }
    } else if msg.role == ChatRole::System {
        "SYSTEM".to_string()
    } else {
        msg.sub_role
            .clone()
            .unwrap_or_else(|| "WORKER".to_string())
            .to_uppercase()
    };

    let header = if !is_user {
        h_stack((
            label(String::new).style(move |s| {
                s.width(6.0)
                    .height(6.0)
                    .border_radius(99.0)
                    .background(role_dot_color)
                    .margin_right(5.0)
            }),
            label(move || role_label_text.clone()).style(move |s| {
                s.font_size(10.0)
                    .font_weight(Weight::BOLD)
                    .color(crate::ownstack_theme::TEXT_HINT)
            }),
        ))
        .style(|s| s.items_center().padding_bottom(6.0).padding_left(2.0))
        .into_any()
    } else {
        empty().into_any()
    };

    let tool_block = if let Some(tool_res) = &msg.tool_result {
        let diff_block = if let Some(target) = &msg.diff_target {
            let chat_for_click = chat_data.clone();
            let target_for_click = target.clone();
            let target_clone = target.clone();
            h_stack((
                h_stack((
                    label(|| "📄 ").style(|s| s.margin_right(4.0).font_size(10.0)),
                    label(move || target_clone.clone()).style(|s| {
                        s.color(crate::ownstack_theme::TEXT).font_size(12.0)
                    }),
                ))
                .style(|s| s.items_center()),
                label(|| "Review Diff")
                    .style(|s| {
                        s.padding_horiz(8.0)
                            .padding_vert(4.0)
                            .border_radius(4.0)
                            .background(crate::ownstack_theme::ACCENT_DIM.multiply_alpha(0.20))
                            .color(crate::ownstack_theme::ACCENT)
                            .font_size(10.0)
                            .font_weight(Weight::SEMIBOLD)
                            .cursor(CursorStyle::Pointer)
                            .hover(|s| {
                                s.background(crate::ownstack_theme::ACCENT_DIM.multiply_alpha(0.39))
                            })
                    })
                    .on_click_stop(move |_| {
                        // Open the changed file's diff view so the user can
                        // review what the agent modified.
                        chat_for_click.common.internal_command.send(
                            InternalCommand::OpenFileChanges {
                                path: PathBuf::from(&target_for_click),
                            },
                        );
                    }),
            ))
            .style(|s| {
                s.width_full()
                    .justify_between()
                    .items_center()
                    .margin_top(8.0)
                    .padding(8.0)
                    .border(1.0)
                    .border_radius(4.0)
                    .border_color(crate::ownstack_theme::BORDER)
                    .background(crate::ownstack_theme::SURFACE_0)
                    .hover(|s| s.border_color(crate::ownstack_theme::BORDER_STRONG))
            })
            .into_any()
        } else {
            empty().into_any()
        };

        let tool_res_clone = tool_res.clone();
        let diff_block = diff_block.into_any();
        v_stack((
            h_stack((
                label(|| "✓").style(|s| {
                    s.color(crate::ownstack_theme::STATE_OK)
                        .font_weight(Weight::BOLD)
                        .margin_right(6.0)
                }),
                label(move || tool_res_clone.clone()).style(|s| {
                    s.color(crate::ownstack_theme::STATE_OK.multiply_alpha(0.90))
                        .font_size(12.0)
                        .font_weight(Weight::SEMIBOLD)
                }),
            ))
            .style(|s| s.items_center()),
            diff_block,
        ))
        .style(|s| {
            s.width_full()
                .margin_top(12.0)
                .padding_top(12.0)
                .border_top(1.0)
                .border_color(crate::ownstack_theme::BORDER)
        })
        .into_any()
    } else {
        empty().into_any()
    };

    let main_content = v_stack((
        header,
        text(msg.content.clone()).style(move |s| {
            s.width_full()
                .color(text_color)
                .line_height(1.6)
                .font_size(13.0)
        }),
        tool_block,
        dyn_stack(
            move || {
                if let Some(diff) = &msg.diff_content {
                    vec![diff.clone()]
                } else {
                    vec![]
                }
            },
            |diff| diff.clone(),
            {
                let chat_data = chat_data.clone();
                move |diff| diff_view(diff, config, chat_data.clone())
            },
        ),
    ))
    .style(move |s| {
        let align = if is_user {
            floem::style::AlignItems::FlexEnd
        } else {
            floem::style::AlignItems::FlexStart
        };
        let s = s
            .padding(14.0)
            .border_top_left_radius(if is_user { 12.0 } else { 4.0 })
            .border_top_right_radius(if is_user { 4.0 } else { 12.0 })
            .border_bottom_left_radius(12.0)
            .border_bottom_right_radius(12.0)
            .background(bubble_bg)
            .border(if is_user { 0.0 } else { 1.0 })
            .border_color(bubble_border)
            .align_items(align);

        if is_user {
            s.margin_left(40.0)
        } else {
            s.width_full()
        }
    });

    v_stack((main_content,)).style(move |s| {
        let s = s.width_full().padding_bottom(12.0);
        if is_user {
            s.items_end()
        } else {
            s.items_start()
        }
    })
}

fn diff_view(
    diff: String,
    config: floem::reactive::ReadSignal<Arc<crate::config::LapceConfig>>,
    chat_data: OwnStackChatData,
) -> impl View {
    let lines: Vec<String> = diff.lines().map(|s| s.to_string()).collect();
    let diff_id = format!("diff-{}", diff.len());
    let diff_id_accept = diff_id.clone();
    let diff_id_reject = diff_id.clone();
    let decided = create_rw_signal(Option::<bool>::None);

    v_stack((
        label(move || "Proposed Changes:")
            .style(|s| s.font_weight(Weight::BOLD).padding_bottom(4.0)),
        v_stack((dyn_stack(
            move || lines.clone(),
            |line| line.clone(),
            move |line| {
                let (bg_color, text_color) = if line.starts_with('+') {
                    (
                        Some(crate::ownstack_theme::DIFF_ADD_BG),
                        Some(crate::ownstack_theme::DIFF_ADD_TEXT),
                    )
                } else if line.starts_with('-') {
                    (
                        Some(crate::ownstack_theme::DIFF_REMOVE_BG),
                        Some(crate::ownstack_theme::DIFF_REMOVE_TEXT),
                    )
                } else if line.starts_with("@@") {
                    (
                        Some(crate::ownstack_theme::DIFF_HUNK_BG),
                        Some(crate::ownstack_theme::DIFF_HUNK_TEXT),
                    )
                } else {
                    (None, None)
                };

                label(move || line.clone()).style(move |s| {
                    let s = s
                        .width_full()
                        .font_family("monospace".to_string())
                        .font_size(11.0);
                    let s = if let Some(c) = bg_color {
                        s.background(c)
                    } else {
                        s
                    };
                    if let Some(c) = text_color {
                        s.color(c)
                    } else {
                        s
                    }
                })
            },
        ),))
        .style(move |s| {
            let config = config.get();
            s.width_full()
                .padding(8.0)
                .background(config.color(LapceColor::PANEL_BACKGROUND))
                .border(1.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER))
                .border_radius(4.0)
        }),
        // Accept / Reject buttons
        h_stack((
            label(move || {
                match decided.get() {
                    Some(true) => "Accepted".to_string(),
                    Some(false) => "Rejected".to_string(),
                    None => "Accept".to_string(),
                }
            })
            .style(move |s| {
                let done = decided.get().is_some();
                s.padding_horiz(12.0)
                    .padding_vert(4.0)
                    .border_radius(6.0)
                    .font_size(11.0)
                    .font_weight(Weight::BOLD)
                    .cursor(if done { CursorStyle::Default } else { CursorStyle::Pointer })
                    .color(crate::ownstack_theme::STATE_OK)
                    .background(crate::ownstack_theme::STATE_OK.multiply_alpha(0.15))
                    .border(1.0)
                    .border_color(crate::ownstack_theme::STATE_OK.multiply_alpha(0.4))
                    .apply_if(done, |s| s.apply_if(decided.get() != Some(true), |s| s.hide()))
            })
            .on_click_stop({
                let chat = chat_data.clone();
                let id = diff_id_accept;
                move |_| {
                    if decided.get_untracked().is_none() {
                        decided.set(Some(true));
                        chat.send_decision("accept", &id);
                    }
                }
            }),
            label(move || {
                match decided.get() {
                    Some(false) => "Rejected".to_string(),
                    _ => "Reject".to_string(),
                }
            })
            .style(move |s| {
                let done = decided.get().is_some();
                s.padding_horiz(12.0)
                    .padding_vert(4.0)
                    .border_radius(6.0)
                    .font_size(11.0)
                    .font_weight(Weight::BOLD)
                    .cursor(if done { CursorStyle::Default } else { CursorStyle::Pointer })
                    .color(crate::ownstack_theme::STATE_ERROR)
                    .background(crate::ownstack_theme::STATE_ERROR.multiply_alpha(0.15))
                    .border(1.0)
                    .border_color(crate::ownstack_theme::STATE_ERROR.multiply_alpha(0.4))
                    .apply_if(done, |s| s.apply_if(decided.get() != Some(false), |s| s.hide()))
            })
            .on_click_stop({
                let chat = chat_data.clone();
                let id = diff_id_reject;
                move |_| {
                    if decided.get_untracked().is_none() {
                        decided.set(Some(false));
                        chat.send_decision("reject", &id);
                    }
                }
            }),
        ))
        .style(|s| s.gap(8.0).padding_top(6.0)),
    ))
    .style(|s| s.width_full().padding_top(8.0))
}

fn shortcut_hint(key: &'static str, action: &'static str) -> impl View {
    h_stack((
        label(move || key).style(|s| {
            s.font_size(10.0)
                .padding_horiz(5.0)
                .padding_vert(1.0)
                .border(1.0)
                .border_radius(3.0)
                .border_color(crate::ownstack_theme::BORDER)
                .color(crate::ownstack_theme::TEXT_HINT)
                .font_family("monospace".to_string())
        }),
        label(move || action).style(|s| {
            s.font_size(10.0)
                .color(crate::ownstack_theme::TEXT_DIM)
        }),
    ))
    .style(|s| s.items_center().gap(4.0))
}

fn chrono_now() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

fn extract_code_block(content: &str) -> Option<String> {
    let mut in_block = false;
    let mut block_content = String::new();

    for line in content.lines() {
        if line.trim().starts_with("```") {
            if in_block {
                return Some(block_content);
            } else {
                in_block = true;
                continue;
            }
        }
        if in_block {
            block_content.push_str(line);
            block_content.push('\n');
        }
    }

    if !block_content.is_empty() {
        Some(block_content)
    } else {
        None
    }
}

fn extract_structured_patch(content: &str) -> Option<PatchSuggestion> {
    let mut in_patch_block = false;
    let mut pending_path: Option<String> = None;
    let mut body = String::new();

    for line in content.lines() {
        let trimmed = line.trim();

        if !in_patch_block {
            if let Some(rest) = trimmed.strip_prefix("```ownstack_patch") {
                in_patch_block = true;
                let inline_path = rest
                    .trim()
                    .strip_prefix("path=")
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty());
                pending_path = inline_path;
            }
            continue;
        }

        if trimmed.starts_with("```") {
            break;
        }

        if pending_path.is_none() {
            if let Some(path) = trimmed.strip_prefix("path:") {
                let path = path.trim();
                if !path.is_empty() {
                    pending_path = Some(path.to_string());
                    continue;
                }
            }
        }

        body.push_str(line);
        body.push('\n');
    }

    let path = pending_path?;
    if body.trim().is_empty() {
        return None;
    }

    Some(PatchSuggestion {
        path,
        new_content: body,
    })
}

fn is_safe_workspace_relative_path(path: &std::path::Path) -> bool {
    if path.is_absolute() {
        return false;
    }
    !path.components().any(|c| {
        matches!(
            c,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    })
}

fn render_unified_diff(path: &str, old_content: &str, new_content: &str) -> String {
    let old_lines: Vec<String> =
        old_content.lines().map(|l| l.to_string()).collect();
    let new_lines: Vec<String> =
        new_content.lines().map(|l| l.to_string()).collect();
    let ops = compute_line_diff_ops(&old_lines, &new_lines);

    let mut out = Vec::with_capacity(ops.len() + 3);
    out.push(format!("--- a/{}", path));
    out.push(format!("+++ b/{}", path));
    out.push(format!(
        "@@ -1,{} +1,{} @@",
        old_lines.len(),
        new_lines.len()
    ));

    for op in ops {
        match op {
            DiffOp::Keep(line) => out.push(format!(" {}", line)),
            DiffOp::Add(line) => out.push(format!("+{}", line)),
            DiffOp::Remove(line) => out.push(format!("-{}", line)),
        }
    }

    out.join("\n")
}

#[derive(Clone, Debug)]
enum DiffOp {
    Keep(String),
    Add(String),
    Remove(String),
}

fn compute_line_diff_ops(old_lines: &[String], new_lines: &[String]) -> Vec<DiffOp> {
    // Keep memory bounded for very large edits.
    let matrix_budget = old_lines.len().saturating_mul(new_lines.len());
    if matrix_budget > 250_000 {
        let mut ops = Vec::new();
        for line in old_lines {
            ops.push(DiffOp::Remove(line.clone()));
        }
        for line in new_lines {
            ops.push(DiffOp::Add(line.clone()));
        }
        return ops;
    }

    let n = old_lines.len();
    let m = new_lines.len();
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];

    for i in (0..n).rev() {
        for j in (0..m).rev() {
            if old_lines[i] == new_lines[j] {
                lcs[i][j] = lcs[i + 1][j + 1] + 1;
            } else {
                lcs[i][j] = lcs[i + 1][j].max(lcs[i][j + 1]);
            }
        }
    }

    let mut i = 0usize;
    let mut j = 0usize;
    let mut ops = Vec::new();
    while i < n && j < m {
        if old_lines[i] == new_lines[j] {
            ops.push(DiffOp::Keep(old_lines[i].clone()));
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            ops.push(DiffOp::Remove(old_lines[i].clone()));
            i += 1;
        } else {
            ops.push(DiffOp::Add(new_lines[j].clone()));
            j += 1;
        }
    }

    while i < n {
        ops.push(DiffOp::Remove(old_lines[i].clone()));
        i += 1;
    }
    while j < m {
        ops.push(DiffOp::Add(new_lines[j].clone()));
        j += 1;
    }

    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_structured_patch_extracts_path_and_body() {
        let content = r#"
Some intro text
```ownstack_patch path=src/lib.rs
fn main() {}
```
"#;
        let patch = extract_structured_patch(content).expect("patch");
        assert_eq!(patch.path, "src/lib.rs");
        assert!(patch.new_content.contains("fn main() {}"));
    }

    #[test]
    fn render_unified_diff_contains_headers_and_changes() {
        let diff = render_unified_diff("src/lib.rs", "a\nb\n", "a\nc\n");
        assert!(diff.contains("--- a/src/lib.rs"));
        assert!(diff.contains("+++ b/src/lib.rs"));
        assert!(diff.contains("-b"));
        assert!(diff.contains("+c"));
    }

    #[test]
    fn workspace_path_guard_rejects_parent_escape() {
        assert!(!is_safe_workspace_relative_path(std::path::Path::new(
            "../secret.txt"
        )));
        assert!(is_safe_workspace_relative_path(std::path::Path::new(
            "src/main.rs"
        )));
    }
}
