use floem::reactive::{RwSignal, create_rw_signal, Scope, SignalGet, SignalUpdate, SignalWith};
use floem::views::{container, label, stack, text, Decorators, dyn_stack, empty};
use floem::style::{AlignItems, CursorStyle, Display, FlexDirection, JustifyContent, Position, Style};
use floem::View;
use floem::event::EventListener;
use std::rc::Rc;
use crate::config::color::LapceColor;

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
    pub fn new() -> Self {
        Self {
            active: create_rw_signal(false),
            current_step: create_rw_signal(0),
            completed: create_rw_signal(false),
            chosen_provider: create_rw_signal(String::new()),
            chosen_mode: create_rw_signal("Ask".to_string()),
        }
    }

    /// Check if onboarding should be shown (first launch)
    pub fn should_show(&self) -> bool {
        !self.completed.get_untracked()
    }

    /// Start the onboarding wizard
    pub fn start(&self) {
        self.active.set(true);
        self.current_step.set(0);
    }

    /// Get total number of steps
    pub fn total_steps(&self) -> usize {
        ONBOARDING_STEPS.len()
    }

    /// Get current step info
    pub fn current_step_info(&self) -> &'static OnboardingStep {
        let idx = self.current_step.get_untracked();
        &ONBOARDING_STEPS[idx.min(ONBOARDING_STEPS.len() - 1)]
    }

    /// Advance to next step
    pub fn next(&self) {
        let current = self.current_step.get_untracked();
        if current + 1 < ONBOARDING_STEPS.len() {
            self.current_step.set(current + 1);
        } else {
            self.finish();
        }
    }

    /// Go back to previous step
    pub fn previous(&self) {
        let current = self.current_step.get_untracked();
        if current > 0 {
            self.current_step.set(current - 1);
        }
    }

    /// Complete onboarding
    pub fn finish(&self) {
        self.active.set(false);
        self.completed.set(true);
        tracing::info!(
            "Onboarding completed: provider={}, mode={}",
            self.chosen_provider.get_untracked(),
            self.chosen_mode.get_untracked()
        );
    }

    /// Skip onboarding
    pub fn skip(&self) {
        self.active.set(false);
        self.completed.set(true);
    }
}

/// A step in the onboarding wizard
pub struct OnboardingStep {
    pub title: &'static str,
    pub description: &'static str,
    pub step_type: StepType,
}

pub enum StepType {
    Welcome,
    ProviderSetup,
    ModeSelection,
    WorkspaceConfig,
    Finish,
}

static ONBOARDING_STEPS: &[OnboardingStep] = &[
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

pub fn onboarding_view(data: OnboardingData, config: RwSignal<std::sync::Arc<crate::config::LapceConfig>>) -> impl View {
    let data_c = data.clone();
    
    container(
        container(
            stack((
                // Header
                label(move || data.current_step_info().title.to_string())
                    .style(move |s| {
                        s.font_bold()
                            .font_size(24.0)
                            .margin_bottom(20.0)
                            .color(config.get().color(LapceColor::EDITOR_FOREGROUND))
                    }),
                
                // Description
                label(move || data.current_step_info().description.to_string())
                    .style(move |s| {
                        s.font_size(16.0)
                            .margin_bottom(40.0)
                            .color(config.get().color(LapceColor::EDITOR_DIM))
                    }),

                // Step content
                dyn_stack(
                    move || data.current_step.get(),
                    move |_| data.current_step.get(),
                    move |step| {
                        let data = data.clone();
                        match ONBOARDING_STEPS[step].step_type {
                            StepType::Welcome => {
                                container(label(|| "🚀".to_string()).style(|s| s.font_size(60.0)))
                                    .style(|s| s.items_center().justify_center().width_full())
                                    .into_any()
                            }
                            StepType::ProviderSetup => {
                                stack((
                                    provider_button("OpenRouter", data.clone(), config),
                                    provider_button("Anthropic", data.clone(), config),
                                    provider_button("Local (Ollama)", data.clone(), config),
                                ))
                                .style(|s| s.flex_col().gap(10.0, 10.0).width_full())
                                .into_any()
                            }
                            StepType::ModeSelection => {
                                stack((
                                    mode_button("Ask", "Confirm every action", data.clone(), config),
                                    mode_button("Auto", "Background execution", data.clone(), config),
                                    mode_button("Plan", "Review steps first", data.clone(), config),
                                ))
                                .style(|s| s.flex_col().gap(10.0, 10.0).width_full())
                                .into_any()
                            }
                            _ => empty().into_any()
                        }
                    }
                ).style(|s| s.flex_grow(1.0).width_full()),

                // Navigation
                stack((
                    label(|| "Skip").on_click_stop(move |_| {
                        data_c.skip();
                    }).style(move |s| {
                        s.padding_horiz(20.0)
                            .padding_vert(10.0)
                            .cursor(CursorStyle::Pointer)
                            .color(config.get().color(LapceColor::EDITOR_DIM))
                    }),
                    empty().style(|s| s.flex_grow(1.0)),
                    label(|| "Next").on_click_stop(move |_| {
                        data_c.next();
                    }).style(move |s| {
                        s.padding_horiz(30.0)
                            .padding_vert(10.0)
                            .background(config.get().color(LapceColor::PANEL_BACKGROUND))
                            .border(1.0)
                            .border_color(config.get().color(LapceColor::LAPCE_BORDER))
                            .border_radius(4.0)
                            .cursor(CursorStyle::Pointer)
                            .color(config.get().color(LapceColor::EDITOR_FOREGROUND))
                    }),
                ))
                .style(|s| s.flex_row().items_center().width_full().margin_top(40.0)),
            ))
            .style(move |s| {
                s.flex_col()
                    .items_center()
                    .width(500.0)
                    .padding(40.0)
                    .background(config.get().color(LapceColor::PANEL_BACKGROUND))
                    .border(1.0)
                    .border_color(config.get().color(LapceColor::LAPCE_BORDER))
                    .border_radius(8.0)
            })
        )
        .style(move |s| {
            s.absolute()
                .size_full()
                .items_center()
                .justify_center()
                .background(config.get().color(LapceColor::LAPCE_DROPDOWN_SHADOW).multiply_alpha(0.8))
                .display(if data_c.active.get() { Display::Flex } else { Display::None })
        })
    )
}

fn provider_button(name: &'static str, data: OnboardingData, config: RwSignal<std::sync::Arc<crate::config::LapceConfig>>) -> impl View {
    let name_s = name.to_string();
    let is_selected = move || data.chosen_provider.get() == name_s;
    
    label(move || name_s.clone())
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

fn mode_button(name: &'static str, desc: &'static str, data: OnboardingData, config: RwSignal<std::sync::Arc<crate::config::LapceConfig>>) -> impl View {
    let name_s = name.to_string();
    let is_selected = move || data.chosen_mode.get() == name_s;
    
    stack((
        label(move || name_s.clone()).style(|s| s.font_bold()),
        label(move || desc.to_string()).style(move |s| s.font_size(12.0).color(config.get().color(LapceColor::EDITOR_DIM))),
    ))
    .on_click_stop(move |_| {
        data.chosen_mode.set(name.to_string());
    })
    .style(move |s| {
        let config = config.get();
        s.flex_col()
            .padding(15.0)
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
    })
}
