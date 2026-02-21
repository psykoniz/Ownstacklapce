use crate::app::clickable_icon;
use crate::config::{color::LapceColor, icon::LapceIcons};
use crate::panel::position::PanelPosition;
use floem::prelude::{SignalGet, SignalUpdate};
use floem::reactive::{RwSignal, create_rw_signal};
use floem::{
    View,
    peniko::Color,
    style::CursorStyle,
    views::{
        Decorators, container, dyn_stack, h_stack, label, scroll, stack,
        text, text_input, v_stack,
    },
};
use lapce_rpc::ownstack::OwnStackRpc;
use serde::{Deserialize, Serialize};
use std::rc::Rc;

use crate::window_tab::CommonData;

/// A single message in the AI chat
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    pub timestamp: String,
    // Optional diff content for preview
    pub diff_content: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ChatRole {
    User,
    Assistant,
    System,
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
    pub current_mission: RwSignal<Option<(String, Vec<(String, String)>)>>,
    #[allow(dead_code)]
    common: CommonData,
}

/// Agent execution mode
#[derive(Clone, Debug, PartialEq)]
pub enum AgentMode {
    Ask,
    Auto,
    Plan,
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

impl OwnStackChatData {
    pub fn new(common: CommonData) -> Self {
        Self {
            input: create_rw_signal(String::new()),
            visible: create_rw_signal(false),
            messages: create_rw_signal(Vec::new()),
            is_loading: create_rw_signal(false),
            agent_mode: create_rw_signal(AgentMode::Ask),
            streaming_content: create_rw_signal(String::new()),
            current_mission: create_rw_signal(None),
            common,
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

        // Add user message to history
        let user_msg = ChatMessage {
            role: ChatRole::User,
            content: prompt.clone(),
            timestamp: chrono_now(),
            diff_content: None,
        };

        self.messages.update(|msgs| {
            msgs.push(user_msg);
        });

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
        let assistant_msg = ChatMessage {
            role: ChatRole::Assistant,
            content,
            timestamp: chrono_now(),
            diff_content: diff,
        };
        self.messages.update(|msgs| {
            msgs.push(assistant_msg);
        });
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
                    // Rudimentary diff detection (looking for markdown code blocks)
                    // In a real usage we might restrict this only to ```diff blocks
                    let diff = extract_code_block(&content);
                    self.receive_response(content, diff);
                }
            }
        }
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
    }

    /// Add a system message
    pub fn add_system_message(&self, content: String) {
        let sys_msg = ChatMessage {
            role: ChatRole::System,
            content,
            timestamp: chrono_now(),
            diff_content: None,
        };

        self.messages.update(|msgs| {
            msgs.push(sys_msg);
        });
    }

    /// Clear chat history
    pub fn clear_history(&self) {
        self.messages.set(Vec::new());
    }

    /// Cycle through agent modes: Ask → Auto → Plan → Ask
    pub fn cycle_mode(&self) {
        let current = self.agent_mode.get_untracked();
        let next = match current {
            AgentMode::Ask => AgentMode::Auto,
            AgentMode::Auto => AgentMode::Plan,
            AgentMode::Plan => AgentMode::Ask,
        };
        tracing::info!("Agent mode changed: {:?} → {:?}", current, next);
        self.agent_mode.set(next);
    }

    /// Get message count
    pub fn message_count(&self) -> usize {
        self.messages.get_untracked().len()
    }
}

pub fn ownstack_chat_panel(
    window_tab_data: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let chat_data = window_tab_data.ownstack_chat.clone();
    let config = window_tab_data.common.config;
    let input = chat_data.input;

    v_stack((
        // Header
        h_stack((
            text("OwnStack AI Chat").style(|s| s.font_weight(Weight::BOLD)),
            h_stack((
                label(move || chat_data.agent_mode.get().to_string()).style(
                    move |s| {
                        let config = config.get();
                        s.padding_horiz(6.0)
                            .border(1.0)
                            .border_radius(4.0)
                            .border_color(config.color(LapceColor::LAPCE_BORDER))
                    },
                ),
                clickable_icon(
                    || LapceIcons::SETTINGS,
                    {
                        let chat_data = chat_data.clone();
                        move || chat_data.cycle_mode()
                    },
                    || false,
                    || false,
                    || "Cycle Mode",
                    config,
                ),
            ))
            .style(|s| s.items_center().gap(10.0)),
        ))
        .style(move |s| {
            let config = config.get();
            s.width_full()
                .padding(10.0)
                .justify_between()
                .border_bottom(1.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER))
        }),
        // Messages list
        scroll(
            v_stack((
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
                                s.font_weight(Weight::BOLD).padding_bottom(5.0)
                            }),
                            v_stack((dyn_stack(
                                move || steps.clone(),
                                |(desc, _)| desc.clone(),
                                move |(desc, status)| {
                                    h_stack((
                                        label(move || match status.as_str() {
                                            "Pending" => "○",
                                            "InProgress" => "▶",
                                            "Completed" => "✓",
                                            "Failed(_)" => "✕",
                                            _ => "?",
                                        })
                                        .style(|s| s.width(20.0)),
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
                                    config.color(LapceColor::PANEL_BACKGROUND),
                                )
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
                                },
                                chat_data.clone(),
                                config,
                            )
                        }
                    },
                ),
            ))
            .style(|s| s.width_full().padding(10.0).gap(15.0)),
        )
        .style(|s| s.flex_grow(1.0).width_full()),
        // Input area
        container(
            h_stack((
                text_input(input)
                    .placeholder("Ask OwnStack anything...")
                    .style(move |s| {
                        let config = config.get();
                        s.width_full()
                            .padding(8.0)
                            .border(1.0)
                            .border_radius(6.0)
                            .border_color(config.color(LapceColor::LAPCE_BORDER))
                            .background(config.color(LapceColor::EDITOR_BACKGROUND))
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
                        move |s| s.apply_if(chat_data.is_loading.get(), |s| s.hide())
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
        )
        .style(move |s| {
            let config = config.get();
            s.width_full()
                .padding(10.0)
                .border_top(1.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER))
        }),
    ))
    .style(|s| s.size_full().flex_col())
}

fn message_view(
    msg: ChatMessage,
    chat_data: OwnStackChatData,
    config: floem::reactive::ReadSignal<Arc<crate::config::LapceConfig>>,
) -> impl View {
    let is_user = msg.role == ChatRole::User;

    v_stack((
        h_stack((
            label(move || if is_user { "You" } else { "AI" }).style(move |s| {
                s.font_weight(Weight::BOLD).color(if is_user {
                    Color::from_rgb8(100, 150, 255)
                } else {
                    Color::from_rgb8(100, 255, 150)
                })
            }),
            text(msg.timestamp.clone()).style(move |s| {
                s.font_size(10.0)
                    .color(config.get().color(LapceColor::EDITOR_DIM))
            }),
        ))
        .style(|s| s.items_center().justify_between()),
        v_stack((
            // Message content
            text(msg.content).style(|s| s.width_full().padding_top(4.0)),
            // Optional Diff Preview
            dyn_stack(
                move || {
                    if let Some(diff) = &msg.diff_content {
                        vec![diff.clone()]
                    } else {
                        vec![]
                    }
                },
                |diff| diff.clone(),
                move |diff| {
                    let chat_data_for_accept = chat_data.clone();
                    let chat_data_for_reject = chat_data.clone();
                    let chat_data_for_discuss = chat_data.clone();
                    // We need a unique ID for the message, using timestamp for now
                    let msg_id_accept = msg.timestamp.clone();
                    let msg_id_reject = msg.timestamp.clone();
                    let msg_id_discuss = msg.timestamp.clone();

                    v_stack((
                        diff_view(diff, config),
                        // Decision Buttons
                        h_stack((
                            label(|| "Accept")
                                .style(|s| {
                                    s.padding_horiz(10.0)
                                        .padding_vert(6.0)
                                        .border_radius(4.0)
                                        .background(Color::from_rgb8(50, 150, 50))
                                        .color(Color::WHITE)
                                        .cursor(CursorStyle::Pointer)
                                })
                                .on_click_stop(move |_| {
                                    chat_data_for_accept
                                        .send_decision("accept", &msg_id_accept);
                                }),
                            label(|| "Reject")
                                .style(|s| {
                                    s.padding_horiz(10.0)
                                        .padding_vert(6.0)
                                        .border_radius(4.0)
                                        .background(Color::from_rgb8(150, 50, 50))
                                        .color(Color::WHITE)
                                        .cursor(CursorStyle::Pointer)
                                })
                                .on_click_stop(move |_| {
                                    chat_data_for_reject
                                        .send_decision("reject", &msg_id_reject);
                                }),
                            label(|| "Discuss")
                                .style(|s| {
                                    s.padding_horiz(10.0)
                                        .padding_vert(6.0)
                                        .border_radius(4.0)
                                        .background(Color::from_rgb8(50, 50, 150))
                                        .color(Color::WHITE)
                                        .cursor(CursorStyle::Pointer)
                                })
                                .on_click_stop(move |_| {
                                    chat_data_for_discuss
                                        .send_decision("discuss", &msg_id_discuss);
                                }),
                        ))
                        .style(|s| s.padding_top(8.0).gap(10.0)),
                    ))
                },
            ),
        ))
        .style(move |s| {
            let config = config.get();
            s.width_full()
                .padding(10.0)
                .border_radius(8.0)
                .background(if is_user {
                    config.color(LapceColor::PANEL_BACKGROUND)
                } else {
                    config.color(LapceColor::EDITOR_BACKGROUND)
                })
        }),
    ))
    .style(|s| s.width_full().padding_bottom(10.0))
}

fn diff_view(
    diff: String,
    config: floem::reactive::ReadSignal<Arc<crate::config::LapceConfig>>,
) -> impl View {
    let lines: Vec<String> = diff.lines().map(|s| s.to_string()).collect();

    v_stack((
        label(move || "Proposed Changes:")
            .style(|s| s.font_weight(Weight::BOLD).padding_bottom(4.0)),
        v_stack((dyn_stack(
            move || lines.clone(),
            |line| line.clone(),
            move |line| {
                let (bg_color, text_color) = if line.starts_with('+') {
                    (
                        Some(Color::from_rgba8(0, 255, 0, 30)),
                        Some(Color::from_rgb8(100, 255, 100)),
                    )
                } else if line.starts_with('-') {
                    (
                        Some(Color::from_rgba8(255, 0, 0, 30)),
                        Some(Color::from_rgb8(255, 100, 100)),
                    )
                } else if line.starts_with("@@") {
                    (
                        Some(Color::from_rgba8(0, 0, 255, 30)),
                        Some(Color::from_rgb8(100, 100, 255)),
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
    ))
    .style(|s| s.width_full().padding_top(8.0))
}

fn chrono_now() -> String {
    // Use a simple counter-based approach since we can't easily add chrono
    // In production, this would use proper timestamps
    format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    )
}

fn extract_code_block(content: &str) -> Option<String> {
    let mut lines = content.lines();
    let mut in_block = false;
    let mut block_content = String::new();

    while let Some(line) = lines.next() {
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
