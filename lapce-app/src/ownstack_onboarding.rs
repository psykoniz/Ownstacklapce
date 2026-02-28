use crate::config::LapceConfig;
use crate::config::color::LapceColor;
use floem::{
    IntoView, View,
    event::{Event, EventListener},
    keyboard::{Key, NamedKey},
    peniko::Color,
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate},
    style::CursorStyle,
    text::Weight,
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

    container(container(
        v_stack((
            // ── Progress indicator: Step X / 5 + bar ─────────────────────
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
                label(move || {
                    format!("Step {} / {}", data_progress.current_step.get() + 1, total_steps)
                })
                .style(move |s| {
                    s.font_size(11.0)
                        .color(config.get().color(LapceColor::EDITOR_DIM))
                        .margin_left(8.0)
                }),
            ))
            .style(|s| s.width_full().justify_between().items_center().margin_bottom(4.0)),
            // Progress bar
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
            label(move || data_desc.current_step_info().description.to_string())
                .style(move |s| {
                    s.font_size(13.0)
                        .line_height(1.45)
                        .width_full()
                        .margin_bottom(14.0)
                        .color(config.get().color(LapceColor::EDITOR_DIM))
                }),
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
                    let data_label = data_nav.clone();
                    let data_click = data_nav.clone();
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
                .min_width(360.0)
                .max_width(560.0)
                .width_full()
                .padding_horiz(24.0)
                .padding_vert(20.0)
                .background(config.color(LapceColor::PANEL_BACKGROUND))
                .border(1.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER))
                .border_radius(8.0)
        }),
    ))
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
        label(|| "Recommended workspace files:".to_string()).style(move |s| {
            s.font_bold()
                .color(config.get().color(LapceColor::EDITOR_FOREGROUND))
        }),
        label(|| "- .ownstack/budgets.json".to_string()).style(move |s| {
            s.font_size(12.0)
                .color(config.get().color(LapceColor::EDITOR_DIM))
        }),
        label(|| "- .ownstack/policy.json".to_string()).style(move |s| {
            s.font_size(12.0)
                .color(config.get().color(LapceColor::EDITOR_DIM))
        }),
    ))
    .style(|s| s.width_full().gap(6.0))
}

fn finish_step(
    data: OnboardingData,
    config: RwSignal<Arc<LapceConfig>>,
) -> impl View {
    let backend = keyring_backend_label();
    v_stack((
        label(move || format!("Provider: {}", data.chosen_provider.get())).style(
            move |s| {
                s.font_size(12.0)
                    .color(config.get().color(LapceColor::EDITOR_FOREGROUND))
            },
        ),
        label(move || format!("Mode: {}", data.chosen_mode.get())).style(move |s| {
            s.font_size(12.0)
                .color(config.get().color(LapceColor::EDITOR_FOREGROUND))
        }),
        label(move || format!("Secrets are stored in {backend}.")).style(move |s| {
            s.font_size(12.0)
                .color(config.get().color(LapceColor::EDITOR_DIM))
        }),
    ))
    .style(|s| s.width_full().gap(8.0))
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
    let display_value: RwSignal<String> = floem::reactive::create_rw_signal(String::new());

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
    config: RwSignal<Arc<LapceConfig>>,
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
