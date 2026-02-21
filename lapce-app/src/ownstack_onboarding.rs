use crate::config::color::LapceColor;
use floem::{
    IntoView, View,
    event::{Event, EventListener},
    keyboard::{Key, NamedKey},
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate},
    style::CursorStyle,
    views::{Decorators, container, dyn_stack, empty, h_stack, label, v_stack},
};
use lapce_core::directory::Directory;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

const ONBOARDING_STATE_FILE: &str = "ownstack-onboarding.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OnboardingState {
    completed: bool,
}

/// Onboarding wizard state for first-launch experience
#[derive(Clone)]
pub struct OnboardingData {
    /// Whether the wizard is active
    pub active: RwSignal<bool>,
    /// Current step index
    pub current_step: RwSignal<usize>,
    /// Whether onboarding has been completed before
    pub completed: RwSignal<bool>,
    /// User's chosen LLM provider
    pub chosen_provider: RwSignal<String>,
    /// User's chosen agent mode
    pub chosen_mode: RwSignal<String>,
}

impl OnboardingData {
    pub fn new(cx: Scope) -> Self {
        let completed = Self::load_completed_state();
        Self {
            active: cx.create_rw_signal(false),
            current_step: cx.create_rw_signal(0),
            completed: cx.create_rw_signal(completed),
            chosen_provider: cx.create_rw_signal("OpenRouter".to_string()),
            chosen_mode: cx.create_rw_signal("Ask".to_string()),
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
        self.active.set(false);
        self.completed.set(true);
        Self::save_completed_state(true);
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

    fn state_file_path() -> Option<PathBuf> {
        Some(Directory::config_directory()?.join(ONBOARDING_STATE_FILE))
    }

    fn load_completed_state() -> bool {
        let Some(path) = Self::state_file_path() else {
            return false;
        };
        let Ok(content) = fs::read_to_string(path) else {
            return false;
        };
        serde_json::from_str::<OnboardingState>(&content)
            .map(|state| state.completed)
            .unwrap_or(false)
    }

    fn save_completed_state(completed: bool) {
        let Some(path) = Self::state_file_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                tracing::error!("Failed to create onboarding state dir: {err}");
                return;
            }
        }

        let state = OnboardingState { completed };
        let serialized = match serde_json::to_string(&state) {
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
        description: "A Rust-native IDE with embedded AI agents. Fast, secure, and local-first.",
        step_type: StepType::Welcome,
    },
    OnboardingStep {
        title: "Choose Your AI Provider",
        description: "Select which LLM provider to use for AI assistance.",
        step_type: StepType::ProviderSetup,
    },
    OnboardingStep {
        title: "Agent Mode",
        description: "Choose your default agent mode: Ask (confirm before acting), Auto (act automatically), or Plan (plan first).",
        step_type: StepType::ModeSelection,
    },
    OnboardingStep {
        title: "Workspace Setup",
        description: "Create a .ownstack/ folder in your project to customize agent behavior.",
        step_type: StepType::WorkspaceConfig,
    },
    OnboardingStep {
        title: "Ready to Go!",
        description: "Your IDE is configured. Press Ctrl+Shift+P to open the AI palette.",
        step_type: StepType::Finish,
    },
];

pub fn onboarding_view(
    data: OnboardingData,
    config: RwSignal<std::sync::Arc<crate::config::LapceConfig>>,
) -> impl View {
    let data_nav = data.clone();
    let data_title = data.clone();
    let data_desc = data.clone();
    let data_stack = data.clone();
    let data_active = data.clone();

    container(
        container(
            v_stack((
                // Header
                label(move || data_title.current_step_info().title.to_string())
                    .style(move |s| {
                        s.font_bold()
                            .font_size(18.0)
                            .line_height(1.3)
                            .width_full()
                            .margin_bottom(8.0)
                            .color(config.get().color(LapceColor::EDITOR_FOREGROUND))
                    }),
                // Description
                label(move || data_desc.current_step_info().description.to_string())
                    .style(move |s| {
                        s.font_size(13.0)
                            .line_height(1.45)
                            .width_full()
                            .margin_bottom(14.0)
                            .color(config.get().color(LapceColor::EDITOR_DIM))
                    }),
                // Step content
                dyn_stack(
                    move || std::iter::once(data_stack.current_step.get()),
                    move |step| *step,
                    move |step| {
                        let data = data_stack.clone();
                        let config = config;
                        match ONBOARDING_STEPS[step].step_type {
                            StepType::Welcome => container(
                                label(|| "🚀".to_string())
                                    .style(|s| s.font_size(48.0)),
                            )
                            .style(|s| s.items_center().justify_center().width_full())
                            .into_any(),
                            StepType::ProviderSetup => v_stack((
                                provider_button("OpenRouter", data.clone(), config),
                                provider_button("Anthropic", data.clone(), config),
                                provider_button("Local (Ollama)", data.clone(), config),
                            ))
                            .style(|s| s.flex_col().gap(10.0).width_full())
                            .into_any(),
                            StepType::ModeSelection => v_stack((
                                mode_button("Ask", "Confirm every action", data.clone(), config),
                                mode_button("Auto", "Background execution", data.clone(), config),
                                mode_button("Plan", "Review steps first", data.clone(), config),
                            ))
                            .style(|s| s.flex_col().gap(10.0).width_full())
                            .into_any(),
                            _ => empty().into_any(),
                        }
                    },
                )
                .style(|s| s.width_full()),
                // Navigation
                h_stack((
                    {
                        let data = data_nav.clone();
                        label(|| "Skip")
                            .on_click_stop(move |_| {
                                data.skip();
                            })
                            .style(move |s| {
                                let config = config.get();
                                s.padding_horiz(20.0)
                                    .padding_vert(10.0)
                                    .cursor(CursorStyle::Pointer)
                                    .color(config.color(LapceColor::EDITOR_DIM))
                            })
                    },
                    empty().style(|s| s.flex_grow(1.0)),
                    {
                        let data = data_nav.clone();
                        label(|| "Next")
                            .on_click_stop(move |_| {
                                data.next();
                            })
                            .style(move |s| {
                                let config = config.get();
                                s.padding_horiz(30.0)
                                    .padding_vert(10.0)
                                    .background(config.color(LapceColor::PANEL_BACKGROUND))
                                    .border(1.0)
                                    .border_color(config.color(LapceColor::LAPCE_BORDER))
                                    .border_radius(4.0)
                                    .cursor(CursorStyle::Pointer)
                                    .color(config.color(LapceColor::EDITOR_FOREGROUND))
                            })
                    },
                ))
                .style(|s| s.items_center().width_full().margin_top(16.0)),
            ))
            .style(move |s| {
                let config = config.get();
                s.flex_col()
                    .items_start()
                    .min_width(320.0)
                    .max_width(520.0)
                    .width_full()
                    .padding_horiz(24.0)
                    .padding_vert(20.0)
                    .background(config.color(LapceColor::PANEL_BACKGROUND))
                    .border(1.0)
                    .border_color(config.color(LapceColor::LAPCE_BORDER))
                    .border_radius(8.0)
            }),
        ),
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
        s.absolute()
            .size_pct(100.0, 100.0)
            .items_center()
            .justify_center()
            .apply_if(!data_active.active.get(), |s| s.hide())
            .background(
                config
                    .color(LapceColor::LAPCE_DROPDOWN_SHADOW)
                    .multiply_alpha(0.8),
            )
    })
}

fn provider_button(
    name: &'static str,
    data: OnboardingData,
    config: RwSignal<std::sync::Arc<crate::config::LapceConfig>>,
) -> impl View {
    let is_selected = move || data.chosen_provider.get() == name;

    label(move || name.to_string())
        .on_click_stop(move |_| {
            data.chosen_provider.set(name.to_string());
        })
        .style(move |s| {
            let config = config.get();
            s.padding(15.0)
                .width_full()
                .border(1.0)
                .border_radius(4.0)
                .border_color(if is_selected() {
                    config.color(LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE)
                } else {
                    config.color(LapceColor::LAPCE_BORDER)
                })
                .background(config.color(LapceColor::PANEL_BACKGROUND))
                .cursor(CursorStyle::Pointer)
                .items_center()
        })
}

fn mode_button(
    name: &'static str,
    desc: &'static str,
    data: OnboardingData,
    config: RwSignal<std::sync::Arc<crate::config::LapceConfig>>,
) -> impl View {
    let is_selected = move || data.chosen_mode.get() == name;

    v_stack((
        label(move || name.to_string()).style(|s| s.font_bold()),
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
        s.flex_col()
            .width_full()
            .padding(15.0)
            .border(1.0)
            .border_radius(4.0)
            .border_color(if is_selected() {
                config.color(LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE)
            } else {
                config.color(LapceColor::LAPCE_BORDER)
            })
            .background(config.color(LapceColor::PANEL_BACKGROUND))
            .cursor(CursorStyle::Pointer)
    })
}

