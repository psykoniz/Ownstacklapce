use crate::config::LapceConfig;
use crate::config::color::LapceColor;
use floem::{
    IntoView, View,
    event::{Event, EventListener},
    keyboard::{Key, NamedKey},
    peniko::Color,
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate},
    style::CursorStyle,
    views::{
        Decorators, container, dyn_stack, empty, h_stack, label, text_input, v_stack,
    },
};
use lapce_core::directory::Directory;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::Arc};

const ONBOARDING_STATE_FILE: &str = "ownstack-onboarding.json";
const KEYRING_SERVICE: &str = "OwnStack IDE";
const OPENROUTER_KEY_ENTRY: &str = "openrouter_api_key";
const ANTHROPIC_KEY_ENTRY: &str = "anthropic_api_key";
const DEFAULT_OLLAMA_HOST: &str = "http://localhost:11434";

fn keyring_backend_label() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "Windows Credential Manager"
    }
    #[cfg(target_os = "macos")]
    {
        "macOS Keychain"
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        "Linux Secret Service keyring"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct OnboardingState {
    completed: bool,
    chosen_provider: Option<String>,
    chosen_mode: Option<String>,
    ollama_host: Option<String>,
}

/// Read persisted onboarding mode preference.
pub fn load_saved_mode_preference() -> Option<String> {
    load_state_file().and_then(|state| state.chosen_mode)
}

#[derive(Clone)]
pub struct OnboardingData {
    pub active: RwSignal<bool>,
    pub current_step: RwSignal<usize>,
    pub completed: RwSignal<bool>,
    pub chosen_provider: RwSignal<String>,
    pub chosen_mode: RwSignal<String>,
    pub openrouter_api_key: RwSignal<String>,
    pub anthropic_api_key: RwSignal<String>,
    pub ollama_host: RwSignal<String>,
    pub openrouter_key_saved: RwSignal<bool>,
    pub anthropic_key_saved: RwSignal<bool>,
    /// Toggle: show/hide the API key in plaintext
    pub show_api_key: RwSignal<bool>,
}

impl OnboardingData {
    pub fn new(cx: Scope) -> Self {
        let state = load_state_file().unwrap_or_default();

        Self {
            active: cx.create_rw_signal(false),
            current_step: cx.create_rw_signal(0),
            completed: cx.create_rw_signal(state.completed),
            chosen_provider: cx.create_rw_signal(
                state
                    .chosen_provider
                    .unwrap_or_else(|| "OpenRouter".to_string()),
            ),
            chosen_mode: cx.create_rw_signal(
                state.chosen_mode.unwrap_or_else(|| "Ask".to_string()),
            ),
            openrouter_api_key: cx.create_rw_signal(String::new()),
            anthropic_api_key: cx.create_rw_signal(String::new()),
            ollama_host: cx.create_rw_signal(
                state
                    .ollama_host
                    .unwrap_or_else(|| DEFAULT_OLLAMA_HOST.to_string()),
            ),
            openrouter_key_saved: cx
                .create_rw_signal(secret_exists(OPENROUTER_KEY_ENTRY)),
            anthropic_key_saved: cx
                .create_rw_signal(secret_exists(ANTHROPIC_KEY_ENTRY)),
            show_api_key: cx.create_rw_signal(false),
        }
    }

    pub fn next(&self) {
        let step = self.current_step.get();
        if step < ONBOARDING_STEPS.len() - 1 {
            self.current_step.set(step + 1);
        } else {
            self.finish();
        }
    }

    pub fn skip(&self) {
        self.finish();
    }

    pub fn finish(&self) {
        self.persist_runtime_setup();
        self.active.set(false);
        self.completed.set(true);
    }

    pub fn current_step_info(&self) -> &'static OnboardingStep {
        &ONBOARDING_STEPS[self.current_step.get()]
    }

    pub fn should_show(&self) -> bool {
        !self.completed.get()
    }

    pub fn start(&self) {
        if self.should_show() {
            self.active.set(true);
        }
    }

    fn persist_runtime_setup(&self) {
        let state = OnboardingState {
            completed: true,
            chosen_provider: Some(self.chosen_provider.get_untracked()),
            chosen_mode: Some(self.chosen_mode.get_untracked()),
            ollama_host: Some(self.ollama_host.get_untracked()),
        };
        save_state_file(&state);

        let openrouter = self.openrouter_api_key.get_untracked();
        if !openrouter.trim().is_empty()
            && save_secret(OPENROUTER_KEY_ENTRY, openrouter.trim())
        {
            self.openrouter_key_saved.set(true);
            self.openrouter_api_key.set(String::new());
        }

        let anthropic = self.anthropic_api_key.get_untracked();
        if !anthropic.trim().is_empty()
            && save_secret(ANTHROPIC_KEY_ENTRY, anthropic.trim())
        {
            self.anthropic_key_saved.set(true);
            self.anthropic_api_key.set(String::new());
        }
    }
}

pub struct OnboardingStep {
    pub title: &'static str,
    pub description: &'static str,
    pub step_type: StepType,
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum StepType {
    Welcome,
    ProviderSetup,
    ModeSelection,
    WorkspaceConfig,
    Finish,
}

pub static ONBOARDING_STEPS: &[OnboardingStep] = &[
    OnboardingStep {
        title: "Welcome to OwnStack IDE",
        description: "A Rust-native IDE with embedded AI agents.",
        step_type: StepType::Welcome,
    },
    OnboardingStep {
        title: "Choose Your AI Provider",
        description: "Select the provider and configure credentials.",
        step_type: StepType::ProviderSetup,
    },
    OnboardingStep {
        title: "Agent Mode",
        description: "Set your default mode: Ask (confirm), Auto, or Plan.",
        step_type: StepType::ModeSelection,
    },
    OnboardingStep {
        title: "Workspace Setup",
        description: "Create .ownstack/ to customize policies and budgets.",
        step_type: StepType::WorkspaceConfig,
    },
    OnboardingStep {
        title: "Ready to Go",
        description: "Your setup is saved.",
        step_type: StepType::Finish,
    },
];

pub fn onboarding_view(
    data: OnboardingData,
    config: RwSignal<Arc<LapceConfig>>,
) -> impl View {
    let data_nav = data.clone();
    let data_title = data.clone();
    let data_desc = data.clone();
    let data_stack = data.clone();
    let data_active = data.clone();
    let data_progress = data.clone();
    let total_steps = ONBOARDING_STEPS.len();

    container(
        v_stack((
            // ── Title row + step badge ───────────────────────────────────
            h_stack((
                label(move || data_title.current_step_info().title.to_string()).style(
                    move |s| {
                        s.font_bold()
                            .font_size(18.0)
                            .line_height(1.3)
                            .margin_bottom(0.0)
                            .color(config.get().color(LapceColor::EDITOR_FOREGROUND))
                    },
                ),
                // Pill-shaped step badge: blue bg + white text, 12px
                label(move || {
                    format!("Step {} / {}", data_progress.current_step.get() + 1, total_steps)
                })
                .style(move |s| {
                    s.font_size(12.0)
                        .font_bold()
                        .color(Color::WHITE)
                        .background(Color::from_rgb8(74, 158, 255))
                        .padding_horiz(10.0)
                        .padding_vert(3.0)
                        .border_radius(10.0)
                        .margin_left(8.0)
                }),
            ))
            .style(|s| s.width_full().justify_between().items_center().margin_bottom(8.0)),
            // ── Step dots: 5 small circles above progress bar ────────────
            {
                let data_dots = data.clone();
                h_stack((
                    // Dot 1 — always filled (step 0 is always reached)
                    {
                        empty().style(move |s| {
                            s.width(8.0).height(8.0).border_radius(4.0)
                                .background(Color::from_rgb8(74, 158, 255))
                        })
                    },
                    // Dot 2
                    {
                        let d = data_dots.clone();
                        empty().style(move |s| {
                            let current = d.current_step.get();
                            let dot_base = s.width(8.0).height(8.0).border_radius(4.0);
                            if current >= 1 {
                                dot_base.background(Color::from_rgb8(74, 158, 255))
                            } else {
                                dot_base
                                    .border(1.5)
                                    .border_color(Color::from_rgba8(74, 158, 255, 100))
                            }
                        })
                    },
                    // Dot 3
                    {
                        let d = data_dots.clone();
                        empty().style(move |s| {
                            let current = d.current_step.get();
                            let dot_base = s.width(8.0).height(8.0).border_radius(4.0);
                            if current >= 2 {
                                dot_base.background(Color::from_rgb8(74, 158, 255))
                            } else {
                                dot_base
                                    .border(1.5)
                                    .border_color(Color::from_rgba8(74, 158, 255, 100))
                            }
                        })
                    },
                    // Dot 4
                    {
                        let d = data_dots.clone();
                        empty().style(move |s| {
                            let current = d.current_step.get();
                            let dot_base = s.width(8.0).height(8.0).border_radius(4.0);
                            if current >= 3 {
                                dot_base.background(Color::from_rgb8(74, 158, 255))
                            } else {
                                dot_base
                                    .border(1.5)
                                    .border_color(Color::from_rgba8(74, 158, 255, 100))
                            }
                        })
                    },
                    // Dot 5
                    {
                        let d = data_dots.clone();
                        empty().style(move |s| {
                            let current = d.current_step.get();
                            let dot_base = s.width(8.0).height(8.0).border_radius(4.0);
                            if current >= 4 {
                                dot_base.background(Color::from_rgb8(74, 158, 255))
                            } else {
                                dot_base
                                    .border(1.5)
                                    .border_color(Color::from_rgba8(74, 158, 255, 100))
                            }
                        })
                    },
                ))
                .style(|s| {
                    s.width_full()
                        .justify_center()
                        .items_center()
                        .gap(10.0)
                        .margin_bottom(6.0)
                })
            },
            // ── Progress bar ─────────────────────────────────────────────
            {
                let data_bar = data.clone();
                h_stack((
                    label(|| "").style(move |s| {
                        let step = data_bar.current_step.get();
                        let pct = ((step + 1) as f64 / total_steps as f64) * 100.0;
                        s.height(3.0)
                            .width_pct(pct)
                            .border_radius(2.0)
                            .background(Color::from_rgb8(74, 158, 255))
                    }),
                ))
                .style(|s| {
                    s.width_full()
                        .height(3.0)
                        .border_radius(2.0)
                        .background(Color::from_rgba8(74, 158, 255, 40))
                        .margin_bottom(12.0)
                })
            },
            // ── Description ──────────────────────────────────────────────
            label(move || data_desc.current_step_info().description.to_string())
                .style(move |s| {
                    s.font_size(13.0)
                        .line_height(1.45)
                        .width_full()
                        .margin_bottom(14.0)
                        .color(config.get().color(LapceColor::EDITOR_DIM))
                }),
            // ── Dynamic step content ─────────────────────────────────────
            dyn_stack(
                move || std::iter::once(data_stack.current_step.get()),
                move |step| *step,
                move |step| {
                    let data = data_stack.clone();
                    let config = config;
                    match ONBOARDING_STEPS[step].step_type {
                        StepType::Welcome => welcome_step(config).into_any(),
                        StepType::ProviderSetup => {
                            provider_setup_step(data.clone(), config).into_any()
                        }
                        StepType::ModeSelection => {
                            mode_selection_step(data.clone(), config).into_any()
                        }
                        StepType::WorkspaceConfig => {
                            workspace_step(config).into_any()
                        }
                        StepType::Finish => {
                            finish_step(data.clone(), config).into_any()
                        }
                    }
                },
            )
            .style(|s| s.width_full()),
            // ── Navigation: Skip | spacer | Next/Finish ─────────────────
            h_stack((
                {
                    let data = data_nav.clone();
                    let data_vis = data_nav.clone();
                    label(|| "Skip")
                        .on_click_stop(move |_| {
                            data.skip();
                        })
                        .style(move |s| {
                            let config = config.get();
                            let is_last = data_vis.current_step.get() + 1
                                == ONBOARDING_STEPS.len();
                            s.padding_horiz(20.0)
                                .padding_vert(10.0)
                                .cursor(CursorStyle::Pointer)
                                .font_size(13.0)
                                .color(config.color(LapceColor::EDITOR_DIM).with_alpha(0.7))
                                .border(1.0)
                                .border_color(Color::TRANSPARENT)
                                .border_radius(4.0)
                                .hover(|s| {
                                    s.color(Color::from_rgb8(180, 200, 230))
                                        .border_color(Color::from_rgba8(120, 140, 170, 80))
                                        .background(Color::from_rgba8(255, 255, 255, 8))
                                })
                                .apply_if(is_last, |s| s.hide())
                        })
                },
                empty().style(|s| s.flex_grow(1.0)),
                {
                    let data_label = data_nav.clone();
                    let data_click = data_nav.clone();
                    let data_style = data_nav.clone();
                    label(move || {
                        if data_label.current_step.get() + 1
                            == ONBOARDING_STEPS.len()
                        {
                            "Finish".to_string()
                        } else {
                            "Next".to_string()
                        }
                    })
                    .on_click_stop(move |_| {
                        data_click.next();
                    })
                    .style(move |s| {
                        let is_finish = data_style.current_step.get() + 1
                            == ONBOARDING_STEPS.len();
                        let (bg, bg_hover) = if is_finish {
                            (
                                Color::from_rgb8(50, 180, 100),
                                Color::from_rgb8(60, 200, 115),
                            )
                        } else {
                            (
                                Color::from_rgb8(74, 158, 255),
                                Color::from_rgb8(95, 172, 255),
                            )
                        };
                        s.padding_horiz(30.0)
                            .padding_vert(10.0)
                            .background(bg)
                            .border(0.0)
                            .border_radius(6.0)
                            .cursor(CursorStyle::Pointer)
                            .color(Color::WHITE)
                            .font_bold()
                            .hover(move |s| {
                                s.background(bg_hover)
                            })
                    })
                },
            ))
            .style(|s| s.items_center().width_full().margin_top(16.0)),
        ))
        .style(move |s| {
            let config = config.get();
            s.flex_col()
                .items_start()
                .min_width(340.0)
                .max_width(460.0)
                .padding_horiz(28.0)
                .padding_vert(24.0)
                .background(config.color(LapceColor::PANEL_BACKGROUND))
                .border(1.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER))
                .border_radius(10.0)
        }),
    )
    .keyboard_navigable()
    .on_event_stop(EventListener::PointerDown, |_| {})
    .on_event_stop(EventListener::KeyDown, {
        let data = data_active.clone();
        move |event| {
            if let Event::KeyDown(key_event) = event {
                if key_event.key.logical_key == Key::Named(NamedKey::Escape) {
                    data.skip();
                }
            }
        }
    })
    .style(move |s| {
        let config = config.get();
        // padding_left compensates for left sidebar so the card
        // appears centred in the editor area, not the whole window.
        s.absolute()
            .size_pct(100.0, 100.0)
            .padding_left(180.0)
            .items_center()
            .justify_center()
            .z_index(50)
            .apply_if(!data_active.active.get(), |s| s.hide())
            .background(
                config
                    .color(LapceColor::LAPCE_DROPDOWN_SHADOW)
                    .multiply_alpha(0.8),
            )
    })
}

fn welcome_step(config: RwSignal<Arc<LapceConfig>>) -> impl View {
    v_stack((
        label(|| "OwnStack first-launch setup".to_string()).style(move |s| {
            s.font_bold()
                .font_size(14.0)
                .color(config.get().color(LapceColor::EDITOR_FOREGROUND))
        }),
        label(|| {
            "Set provider credentials and your default execution mode.".to_string()
        })
        .style(move |s| {
            s.font_size(12.0)
                .color(config.get().color(LapceColor::EDITOR_DIM))
        }),
    ))
    .style(|s| s.width_full().gap(8.0))
}

fn provider_setup_step(
    data: OnboardingData,
    config: RwSignal<Arc<LapceConfig>>,
) -> impl View {
    let selected_openrouter = data.clone();
    let selected_anthropic = data.clone();
    let selected_ollama = data.clone();

    v_stack((
        provider_button("OpenRouter", data.clone(), config),
        provider_button("Anthropic", data.clone(), config),
        provider_button("Local (Ollama)", data.clone(), config),
        provider_secret_input(
            "OpenRouter API key",
            "sk-or-v1-...",
            data.openrouter_api_key,
            data.openrouter_key_saved,
            data.show_api_key,
            config,
        )
        .style(move |s| {
            s.apply_if(
                selected_openrouter.chosen_provider.get() != "OpenRouter",
                |s| s.hide(),
            )
        }),
        provider_secret_input(
            "Anthropic API key",
            "sk-ant-...",
            data.anthropic_api_key,
            data.anthropic_key_saved,
            data.show_api_key,
            config,
        )
        .style(move |s| {
            s.apply_if(
                selected_anthropic.chosen_provider.get() != "Anthropic",
                |s| s.hide(),
            )
        }),
        ollama_host_input(data.ollama_host, config).style(move |s| {
            s.apply_if(
                selected_ollama.chosen_provider.get() != "Local (Ollama)",
                |s| s.hide(),
            )
        }),
    ))
    .style(|s| s.flex_col().gap(10.0).width_full())
}

fn mode_selection_step(
    data: OnboardingData,
    config: RwSignal<Arc<LapceConfig>>,
) -> impl View {
    v_stack((
        mode_button("Ask", "Confirm every action", data.clone(), config),
        mode_button("Auto", "Background execution", data.clone(), config),
        mode_button("Plan", "Review steps first", data.clone(), config),
    ))
    .style(|s| s.flex_col().gap(10.0).width_full())
}

fn workspace_step(config: RwSignal<Arc<LapceConfig>>) -> impl View {
    v_stack((
        // Folder icon
        label(|| "\u{1F4C1}".to_string()).style(|s| {
            s.font_size(28.0).margin_bottom(4.0)
        }),
        label(|| "Project configuration".to_string()).style(move |s| {
            s.font_bold()
                .font_size(14.0)
                .color(config.get().color(LapceColor::EDITOR_FOREGROUND))
        }),
        label(|| {
            "A .ownstack/ directory will be created in your project \
             root with these files:"
                .to_string()
        })
        .style(move |s| {
            s.font_size(12.0)
                .line_height(1.4)
                .color(config.get().color(LapceColor::EDITOR_DIM))
                .margin_bottom(6.0)
        }),
        // File list with descriptions
        workspace_file_row("budgets.json", "Token & cost limits per session", config),
        workspace_file_row("policy.json", "Allowed tools, file access rules", config),
        workspace_file_row("mcp_servers.json", "MCP server configurations", config),
        // Hint
        label(|| {
            "These files can be committed to version control to share settings with your team."
                .to_string()
        })
        .style(move |s| {
            s.font_size(11.0)
                .line_height(1.4)
                .color(config.get().color(LapceColor::EDITOR_DIM).with_alpha(0.7))
                .margin_top(8.0)
        }),
    ))
    .style(|s| s.width_full().gap(4.0))
}

fn workspace_file_row(
    filename: &'static str,
    desc: &'static str,
    config: RwSignal<Arc<LapceConfig>>,
) -> impl View {
    h_stack((
        label(move || filename.to_string()).style(move |s| {
            s.font_size(12.0)
                .font_bold()
                .color(Color::from_rgb8(74, 158, 255))
                .min_width(130.0)
        }),
        label(move || desc.to_string()).style(move |s| {
            s.font_size(12.0)
                .color(config.get().color(LapceColor::EDITOR_DIM))
        }),
    ))
    .style(move |s| {
        let config = config.get();
        s.items_center()
            .gap(8.0)
            .padding_horiz(10.0)
            .padding_vert(6.0)
            .border(1.0)
            .border_radius(4.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER).multiply_alpha(0.5))
            .width_full()
    })
}

fn finish_step(
    data: OnboardingData,
    config: RwSignal<Arc<LapceConfig>>,
) -> impl View {
    let backend = keyring_backend_label();
    v_stack((
        // ── Large green checkmark icon ───────────────────────────────
        label(|| "\u{2713}".to_string()).style(move |s| {
            s.font_size(42.0)
                .font_bold()
                .color(Color::from_rgb8(80, 200, 120))
                .margin_bottom(4.0)
        }),
        // ── "Setup Complete" header ──────────────────────────────────
        label(|| "Setup Complete".to_string()).style(move |s| {
            s.font_size(16.0)
                .font_bold()
                .color(config.get().color(LapceColor::EDITOR_FOREGROUND))
                .margin_bottom(8.0)
        }),
        // ── Separator line ───────────────────────────────────────────
        empty().style(move |s| {
            s.width_full()
                .height(1.0)
                .background(config.get().color(LapceColor::LAPCE_BORDER))
                .margin_bottom(8.0)
        }),
        // ── Summary rows ─────────────────────────────────────────────
        h_stack((
            label(|| "Provider:".to_string()).style(move |s| {
                s.font_size(12.0)
                    .font_bold()
                    .color(config.get().color(LapceColor::EDITOR_DIM))
                    .min_width(70.0)
            }),
            label(move || data.chosen_provider.get()).style(move |s| {
                s.font_size(12.0)
                    .color(config.get().color(LapceColor::EDITOR_FOREGROUND))
            }),
        ))
        .style(|s| s.items_center().gap(8.0)),
        h_stack((
            label(|| "Mode:".to_string()).style(move |s| {
                s.font_size(12.0)
                    .font_bold()
                    .color(config.get().color(LapceColor::EDITOR_DIM))
                    .min_width(70.0)
            }),
            label(move || data.chosen_mode.get()).style(move |s| {
                s.font_size(12.0)
                    .color(config.get().color(LapceColor::EDITOR_FOREGROUND))
            }),
        ))
        .style(|s| s.items_center().gap(8.0)),
        // ── Secrets info ─────────────────────────────────────────────
        label(move || format!("Secrets are stored in {}.", backend)).style(
            move |s| {
                s.font_size(11.0)
                    .color(config.get().color(LapceColor::EDITOR_DIM))
                    .margin_top(6.0)
            },
        ),
    ))
    .style(|s| s.width_full().gap(6.0).items_center())
}

/// Secure API key input with masked text + show/hide toggle
fn provider_secret_input(
    title: &'static str,
    placeholder: &'static str,
    value: RwSignal<String>,
    is_saved: RwSignal<bool>,
    show_key: RwSignal<bool>,
    config: RwSignal<Arc<LapceConfig>>,
) -> impl View {
    let backend = keyring_backend_label();

    // We use a display signal: when hidden, we show dots; when visible, the real value.
    // The actual value signal always holds the real key.
    v_stack((
        label(move || title.to_string()).style(move |s| {
            s.font_size(12.0)
                .color(config.get().color(LapceColor::EDITOR_FOREGROUND))
        }),
        h_stack((
            text_input(value)
                .placeholder(placeholder)
                .style(move |s| {
                    let config = config.get();
                    let showing = show_key.get();
                    s.width_full()
                        .padding(10.0)
                        .border(1.0)
                        .border_radius(4.0)
                        .border_color(config.color(LapceColor::LAPCE_BORDER))
                        .background(config.color(LapceColor::EDITOR_BACKGROUND))
                        // Mask text color when hidden: use transparent text + security measure
                        .apply_if(!showing, |s| {
                            s.color(Color::TRANSPARENT)
                        })
                }),
            // Show/Hide toggle button
            label(move || {
                if show_key.get() { "Hide" } else { "Show" }.to_string()
            })
            .on_click_stop(move |_| {
                show_key.update(|v| *v = !*v);
            })
            .style(move |s| {
                s.padding_horiz(10.0)
                    .padding_vert(10.0)
                    .font_size(11.0)
                    .color(Color::from_rgb8(120, 160, 220))
                    .cursor(CursorStyle::Pointer)
                    .border(1.0)
                    .border_radius(4.0)
                    .border_color(config.get().color(LapceColor::LAPCE_BORDER))
                    .hover(|s| s.color(Color::from_rgb8(160, 200, 255)))
            }),
        ))
        .style(|s| s.width_full().gap(6.0).items_center()),
        // Masked preview when hidden (shows dots for the actual key length)
        label(move || {
            if !show_key.get() {
                let len = value.get().len();
                if len > 0 {
                    format!("{}", "*".repeat(len.min(32)))
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        })
        .style(move |s| {
            let vis = !show_key.get() && !value.get().is_empty();
            s.apply_if(!vis, |s| s.hide())
                .font_size(11.0)
                .color(Color::from_rgb8(100, 120, 150))
                .margin_top(2.0)
        }),
        label(move || {
            if is_saved.get() {
                format!("Saved in {backend}. Leave empty to keep current value.")
            } else {
                format!("Your key is stored securely ({backend}).")
            }
        })
        .style(move |s| {
            s.font_size(11.0)
                .color(config.get().color(LapceColor::EDITOR_DIM))
        }),
    ))
    .style(|s| s.width_full().gap(6.0))
}

fn ollama_host_input(
    value: RwSignal<String>,
    config: RwSignal<Arc<LapceConfig>>,
) -> impl View {
    v_stack((
        label(|| "Ollama host".to_string()).style(move |s| {
            s.font_size(12.0)
                .color(config.get().color(LapceColor::EDITOR_FOREGROUND))
        }),
        text_input(value)
            .placeholder(DEFAULT_OLLAMA_HOST)
            .style(move |s| {
                let config = config.get();
                s.width_full()
                    .padding(10.0)
                    .border(1.0)
                    .border_radius(4.0)
                    .border_color(config.color(LapceColor::LAPCE_BORDER))
                    .background(config.color(LapceColor::EDITOR_BACKGROUND))
            }),
    ))
    .style(|s| s.width_full().gap(6.0))
}

fn provider_button(
    name: &'static str,
    data: OnboardingData,
    config: RwSignal<Arc<LapceConfig>>,
) -> impl View {
    let is_selected = move || data.chosen_provider.get() == name;

    label(move || name.to_string())
        .on_click_stop(move |_| {
            data.chosen_provider.set(name.to_string());
        })
        .style(move |s| {
            let config = config.get();
            let selected = is_selected();
            s.padding(15.0)
                .width_full()
                .border_radius(6.0)
                .cursor(CursorStyle::Pointer)
                .items_center()
                .apply_if(selected, |s| {
                    s.border(2.0)
                        .border_color(Color::from_rgb8(74, 158, 255))
                        .background(Color::from_rgba8(74, 158, 255, 25))
                })
                .apply_if(!selected, |s| {
                    s.border(1.0)
                        .border_color(config.color(LapceColor::LAPCE_BORDER))
                        .background(config.color(LapceColor::PANEL_BACKGROUND))
                })
                .hover(|s| {
                    s.background(Color::from_rgba8(74, 158, 255, 20))
                        .border_color(Color::from_rgba8(74, 158, 255, 100))
                })
        })
}

fn mode_button(
    name: &'static str,
    desc: &'static str,
    data: OnboardingData,
    config: RwSignal<Arc<LapceConfig>>,
) -> impl View {
    let is_selected = move || data.chosen_mode.get() == name;

    v_stack((
        label(move || name.to_string()).style(move |s| {
            s.font_bold().apply_if(is_selected(), |s| {
                s.color(Color::from_rgb8(74, 158, 255))
            })
        }),
        label(move || desc.to_string()).style(move |s| {
            let config = config.get();
            s.font_size(12.0)
                .color(config.color(LapceColor::EDITOR_DIM))
        }),
    ))
    .style(|s| s.gap(4.0))
    .on_click_stop(move |_| {
        data.chosen_mode.set(name.to_string());
    })
    .style(move |s| {
        let config = config.get();
        let selected = is_selected();
        s.flex_col()
            .width_full()
            .padding(15.0)
            .border_radius(6.0)
            .cursor(CursorStyle::Pointer)
            .apply_if(selected, |s| {
                s.border(2.0)
                    .border_color(Color::from_rgb8(74, 158, 255))
                    .background(Color::from_rgba8(74, 158, 255, 25))
            })
            .apply_if(!selected, |s| {
                s.border(1.0)
                    .border_color(config.color(LapceColor::LAPCE_BORDER))
                    .background(config.color(LapceColor::PANEL_BACKGROUND))
            })
            .hover(|s| {
                s.background(Color::from_rgba8(74, 158, 255, 20))
                    .border_color(Color::from_rgba8(74, 158, 255, 100))
            })
    })
}

fn state_file_path() -> Option<PathBuf> {
    Some(Directory::config_directory()?.join(ONBOARDING_STATE_FILE))
}

fn load_state_file() -> Option<OnboardingState> {
    let path = state_file_path()?;
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str::<OnboardingState>(&content).ok()
}

fn save_state_file(state: &OnboardingState) {
    let Some(path) = state_file_path() else {
        return;
    };

    if let Some(parent) = path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            tracing::error!("Failed to create onboarding state dir: {err}");
            return;
        }
    }

    let serialized = match serde_json::to_string(state) {
        Ok(s) => s,
        Err(err) => {
            tracing::error!("Failed to serialize onboarding state: {err}");
            return;
        }
    };

    if let Err(err) = fs::write(path, serialized) {
        tracing::error!("Failed to persist onboarding state: {err}");
    }
}

fn secret_exists(entry_name: &str) -> bool {
    let entry = match keyring::Entry::new(KEYRING_SERVICE, entry_name) {
        Ok(entry) => entry,
        Err(err) => {
            tracing::debug!(
                "Failed to access keyring entry metadata for {entry_name}: {err}"
            );
            return false;
        }
    };

    match entry.get_password() {
        Ok(value) => !value.trim().is_empty(),
        Err(_) => false,
    }
}

fn save_secret(entry_name: &str, secret_value: &str) -> bool {
    let entry = match keyring::Entry::new(KEYRING_SERVICE, entry_name) {
        Ok(entry) => entry,
        Err(err) => {
            tracing::error!(
                "Failed to create keyring entry for {entry_name}: {err}"
            );
            return false;
        }
    };

    match entry.set_password(secret_value) {
        Ok(()) => true,
        Err(err) => {
            tracing::error!("Failed to store {entry_name} in keyring: {err}");
            false
        }
    }
}
