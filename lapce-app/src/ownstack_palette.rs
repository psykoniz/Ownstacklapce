use floem::prelude::{SignalGet, SignalUpdate};
use floem::reactive::{RwSignal, create_rw_signal};
use lapce_rpc::ownstack::OwnStackRpc;

use crate::command::LapceWorkbenchCommand;
use crate::window_tab::CommonData;

#[derive(Clone)]
enum SuggestedActionKind {
    Prompt(&'static str),
    Workbench(LapceWorkbenchCommand),
}

#[derive(Clone)]
struct SuggestedAction {
    title: &'static str,
    keywords: &'static [&'static str],
    action: SuggestedActionKind,
}

fn suggested_actions() -> Vec<SuggestedAction> {
    vec![
        // ── Analysis & Review ────────────────────────────────────────
        SuggestedAction {
            title: "Analyze Active File",
            keywords: &["analyze", "active", "file", "review"],
            action: SuggestedActionKind::Prompt(
                "Analyze the active file and summarize key risks and improvements.",
            ),
        },
        SuggestedAction {
            title: "Request Code Review",
            keywords: &["review", "code", "reviewer", "pr", "pull request"],
            action: SuggestedActionKind::Prompt(
                "Review the current file for bugs, security issues, and code quality. Provide actionable suggestions.",
            ),
        },
        SuggestedAction {
            title: "Security Audit",
            keywords: &["security", "audit", "vulnerability", "owasp", "cve"],
            action: SuggestedActionKind::Prompt(
                "Perform a security audit on the current workspace. Check for OWASP top-10 vulnerabilities, hardcoded secrets, and unsafe patterns.",
            ),
        },
        // ── Time Machine ─────────────────────────────────────────────
        SuggestedAction {
            title: "Create Snapshot",
            keywords: &["snapshot", "save", "time", "machine", "backup", "checkpoint"],
            action: SuggestedActionKind::Prompt(
                "Create a Time Machine snapshot of the current workspace state before making changes.",
            ),
        },
        SuggestedAction {
            title: "List Snapshots",
            keywords: &["snapshot", "list", "time", "machine", "history", "restore"],
            action: SuggestedActionKind::Prompt(
                "List recent Time Machine snapshots so I can review or restore a previous state.",
            ),
        },
        SuggestedAction {
            title: "Restore Last Snapshot",
            keywords: &["restore", "undo", "rollback", "revert", "time", "snapshot"],
            action: SuggestedActionKind::Prompt(
                "Restore the workspace to the most recent Time Machine snapshot.",
            ),
        },
        // ── Self-Healing ─────────────────────────────────────────────
        SuggestedAction {
            title: "Auto-Heal: Fix Failing Tests",
            keywords: &["heal", "fix", "test", "auto", "repair", "debug"],
            action: SuggestedActionKind::Prompt(
                "Run the test suite, detect failures, and automatically attempt to fix them using the Healer agent.",
            ),
        },
        SuggestedAction {
            title: "Auto-Heal: Fix Build Errors",
            keywords: &["heal", "build", "compile", "error", "fix", "cargo", "npm"],
            action: SuggestedActionKind::Prompt(
                "Run the build command, detect compilation errors, and automatically attempt fixes.",
            ),
        },
        // ── Multivers (A/B Testing) ──────────────────────────────────
        SuggestedAction {
            title: "Run A/B Test",
            keywords: &["multivers", "ab", "test", "compare", "variant", "fork"],
            action: SuggestedActionKind::Prompt(
                "Run the current command with multiple variant configurations in parallel and compare results using Multivers.",
            ),
        },
        // ── Specialists ──────────────────────────────────────────────
        SuggestedAction {
            title: "Generate Documentation",
            keywords: &["docs", "documentation", "generate", "readme", "jsdoc", "rustdoc"],
            action: SuggestedActionKind::Prompt(
                "Generate documentation for the current file or module using the Docs specialist agent.",
            ),
        },
        SuggestedAction {
            title: "QA: Analyze Test Failures",
            keywords: &["qa", "test", "failure", "analyze", "debug", "coverage"],
            action: SuggestedActionKind::Prompt(
                "Analyze recent test failures and suggest fixes using the QA specialist agent.",
            ),
        },
        SuggestedAction {
            title: "UI/UX Review",
            keywords: &["designer", "ui", "ux", "design", "layout", "css", "style"],
            action: SuggestedActionKind::Prompt(
                "Review the UI components in the current file for accessibility, responsiveness, and design best practices.",
            ),
        },
        SuggestedAction {
            title: "Project Planning",
            keywords: &["pm", "plan", "project", "task", "milestone", "estimate"],
            action: SuggestedActionKind::Prompt(
                "Help plan the next steps for this project. Break down the current task into milestones and estimate effort.",
            ),
        },
        // ── Browser & Vision ─────────────────────────────────────────
        SuggestedAction {
            title: "Browse URL",
            keywords: &["browser", "url", "web", "navigate", "fetch", "scrape"],
            action: SuggestedActionKind::Prompt(
                "Navigate to a URL and analyze the page content. Usage: provide the URL in your message.",
            ),
        },
        SuggestedAction {
            title: "Capture UI Snapshot",
            keywords: &["vision", "capture", "ui", "screenshot", "snapshot"],
            action: SuggestedActionKind::Workbench(
                LapceWorkbenchCommand::OwnStackCaptureUiSnapshot,
            ),
        },
        // ── InfraSense ───────────────────────────────────────────────
        SuggestedAction {
            title: "System Health Check",
            keywords: &["health", "system", "ram", "disk", "cpu", "infra", "metrics"],
            action: SuggestedActionKind::Prompt(
                "Check system health: RAM usage, disk space, and CPU. Alert if any resource is critical.",
            ),
        },
        // ── Policy & Config ──────────────────────────────────────────
        SuggestedAction {
            title: "Simulate Policy: npm publish",
            keywords: &["policy", "npm", "publish", "security"],
            action: SuggestedActionKind::Prompt(
                "Simulate policy evaluation for command `npm publish` and explain the decision.",
            ),
        },
        SuggestedAction {
            title: "Open Settings",
            keywords: &["settings", "preferences", "config"],
            action: SuggestedActionKind::Workbench(
                LapceWorkbenchCommand::OpenSettings,
            ),
        },
    ]
}

fn action_matches_query(action: &SuggestedAction, query: &str) -> bool {
    if query.trim().is_empty() {
        return true;
    }
    let q = query.to_ascii_lowercase();
    action.title.to_ascii_lowercase().contains(&q)
        || action.keywords.iter().any(|k| k.contains(&q))
}

#[derive(Clone)]
pub struct OwnStackPaletteData {
    pub input: RwSignal<String>,
    pub active: RwSignal<bool>,
    common: CommonData,
}

impl OwnStackPaletteData {
    pub fn new(common: CommonData) -> Self {
        Self {
            input: create_rw_signal(String::new()),
            active: create_rw_signal(false),
            common,
        }
    }

    pub fn show(&self) {
        self.active.set(true);
        self.input.set(String::new());
    }

    pub fn hide(&self) {
        self.active.set(false);
    }

    pub fn submit(&self) {
        let prompt = self.input.get_untracked();
        if prompt.is_empty() {
            return;
        }

        self.send_prompt(prompt);
    }

    fn send_prompt(&self, prompt: String) {
        let message = OwnStackRpc::AiPrompt { prompt };
        self.common.proxy.ownstack(message);
        tracing::info!("OwnStack Palette: AiPrompt sent");
        self.hide();
    }

    fn apply_suggested_action(&self, action: SuggestedAction) {
        match action.action {
            SuggestedActionKind::Prompt(prompt) => {
                self.send_prompt(prompt.to_string());
            }
            SuggestedActionKind::Workbench(command) => {
                self.common.workbench_command.send(command);
                self.hide();
            }
        }
    }
}

/// AI Command Palette — full-screen overlay with click-outside dismiss.
/// The overlay catches pointer events outside the palette card to close it.
pub fn ownstack_palette_view(palette_data: OwnStackPaletteData) -> impl floem::View {
    use floem::peniko::Color;
    use floem::prelude::SignalGet;
    use floem::style::{CursorStyle, Display, Position};
    use floem::text::Weight;
    use floem::views::{
        Decorators, container, dyn_stack, h_stack, label, text, text_input, v_stack,
    };

    let active = palette_data.active;
    let input = palette_data.input;

    // ── Palette card content ─────────────────────────────────────────────
    let palette_card = v_stack((
        // Row 1: header with icon + title + shortcut hints
        v_stack((
            h_stack((
                text("\u{27D0}").style(|s| {
                    s.font_size(18.0)
                        .color(crate::ownstack_theme::ACCENT)
                        .margin_right(8.0)
                }),
                text("AI Command Palette").style(|s| {
                    s.font_size(15.0)
                        .font_weight(Weight::BOLD)
                        .color(crate::ownstack_theme::TEXT)
                }),
                // Spacer
                label(|| "").style(|s| s.flex_grow(1.0)),
                label(|| "Esc").style(|s| {
                    s.font_size(10.0)
                        .padding_horiz(6.0)
                        .padding_vert(2.0)
                        .border(1.0)
                        .border_radius(4.0)
                        .border_color(crate::ownstack_theme::BORDER)
                        .color(crate::ownstack_theme::TEXT_HINT)
                        .margin_right(4.0)
                }),
                label(|| "Enter").style(|s| {
                    s.font_size(10.0)
                        .padding_horiz(6.0)
                        .padding_vert(2.0)
                        .border(1.0)
                        .border_radius(4.0)
                        .border_color(crate::ownstack_theme::BORDER)
                        .color(crate::ownstack_theme::TEXT_HINT)
                }),
            ))
            .style(|s| s.items_center().width_full()),
            // Separator line under the header
            label(|| "").style(|s| {
                s.width_full()
                    .height(1.0)
                    .margin_top(10.0)
                    .background(crate::ownstack_theme::ACCENT.multiply_alpha(0.16))
            }),
        ))
        .style(|s| s.width_full().padding_bottom(12.0)),
        // Row 2: input + send button
        h_stack((
            text_input(input)
                .placeholder("Ask AI anything…")
                .style(|s| {
                    s.width_full()
                        .padding(10.0)
                        .border(1.5)
                        .border_radius(10.0)
                        .border_color(crate::ownstack_theme::BORDER_STRONG)
                        .background(crate::ownstack_theme::SURFACE_1)
                        .color(Color::WHITE)
                        .font_size(13.0)
                        .box_shadow_blur(8.0)
                        .box_shadow_color(crate::ownstack_theme::ACCENT.multiply_alpha(0.16))
                })
                .on_event_stop(floem::event::EventListener::KeyDown, {
                    let pd = palette_data.clone();
                    move |event| {
                        if let floem::event::Event::KeyDown(ke) = event {
                            use floem::keyboard::{Key, NamedKey};
                            match &ke.key.logical_key {
                                Key::Named(NamedKey::Enter) => pd.submit(),
                                Key::Named(NamedKey::Escape) => pd.hide(),
                                _ => {}
                            }
                        }
                    }
                }),
            label(|| "➜")
                .style(|s| {
                    s.padding_horiz(16.0)
                        .padding_vert(10.0)
                        .border_radius(10.0)
                        .background(crate::ownstack_theme::ACCENT)
                        .color(Color::WHITE)
                        .font_size(14.0)
                        .cursor(CursorStyle::Pointer)
                        .hover(|s| {
                            s.background(crate::ownstack_theme::ACCENT_BRIGHT)
                                .box_shadow_blur(12.0)
                                .box_shadow_color(crate::ownstack_theme::ACCENT.multiply_alpha(0.47))
                        })
                })
                .on_click_stop({
                    let pd = palette_data.clone();
                    move |_| pd.submit()
                }),
        ))
        .style(|s| s.width_full().items_center().gap(12.0)),
        // Row 3: suggested quick actions with basic filtering + "No results" state.
        v_stack((
            text("Suggested Actions").style(|s| {
                s.font_size(10.0)
                    .font_weight(Weight::BOLD)
                    .color(crate::ownstack_theme::TEXT_DIM)
                    .padding_top(12.0)
                    .padding_bottom(6.0)
                    .border_top(1.0)
                    .border_color(crate::ownstack_theme::ACCENT.multiply_alpha(0.10))
            }),
            dyn_stack(
                {
                    let palette_data = palette_data.clone();
                    move || {
                        let query = palette_data.input.get();
                        suggested_actions()
                            .into_iter()
                            .filter(|action| action_matches_query(action, &query))
                            .collect::<Vec<_>>()
                    }
                },
                |action| action.title.to_string(),
                {
                    let palette_data = palette_data.clone();
                    move |action| {
                        let on_click_data = palette_data.clone();
                        let action_for_click = action.clone();
                        h_stack((
                            text("+").style(|s| {
                                s.margin_right(8.0)
                                    .color(crate::ownstack_theme::ACCENT)
                            }),
                            text(action.title),
                        ))
                        .on_click_stop(move |_| {
                            on_click_data
                                .apply_suggested_action(action_for_click.clone());
                        })
                        .style(move |s| {
                            s.padding_horiz(10.0)
                                .padding_vert(7.0)
                                .border_radius(8.0)
                                .background(crate::ownstack_theme::SURFACE_2.multiply_alpha(0.47))
                                .color(crate::ownstack_theme::TEXT)
                                .font_size(11.5)
                                .cursor(CursorStyle::Pointer)
                                .hover(|s| {
                                    s.background(crate::ownstack_theme::ACCENT.multiply_alpha(0.39))
                                    .color(Color::WHITE)
                                })
                        })
                    }
                },
            ),
            // "No results" label — shown when query filters out everything
            label(move || {
                let query = input.get();
                let count = suggested_actions()
                    .iter()
                    .filter(|a| action_matches_query(a, &query))
                    .count();
                if count == 0 && !query.trim().is_empty() {
                    "No matching actions — press Enter to send as prompt".to_string()
                } else {
                    String::new()
                }
            })
            .style(move |s| {
                let query = input.get();
                let count = suggested_actions()
                    .iter()
                    .filter(|a| action_matches_query(a, &query))
                    .count();
                let visible = count == 0 && !query.trim().is_empty();
                s.apply_if(!visible, |s| s.hide())
                    .padding_vert(10.0)
                    .font_size(11.0)
                    .color(crate::ownstack_theme::TEXT_HINT)
            }),
        ))
        .style(|s| {
            s.width_full()
                .gap(6.0)
                .flex_wrap(floem::style::FlexWrap::Wrap)
        }),
        // Final Hint
        text("Tip: try '/plan' to switch agent to planning mode").style(|s| {
            s.font_size(10.0)
                .color(crate::ownstack_theme::TEXT_HINT)
                .padding_top(12.0)
        }),
    ))
    // Prevent click-through: clicks on the card stay inside the card
    .on_event_stop(floem::event::EventListener::PointerDown, |_| {})
    .style(|s| {
        s.margin_top(64.0)
            .max_width(640.0)
            .width_pct(90.0)
            .padding(18.0)
            .background(crate::ownstack_theme::SURFACE_0.multiply_alpha(0.94))
            .border(1.5)
            .border_color(crate::ownstack_theme::BORDER_STRONG)
            .border_radius(16.0)
            .box_shadow_blur(40.0)
            .box_shadow_color(Color::from_rgba8(0, 0, 0, 180))
    });

    // ── Full-screen overlay (click-outside backdrop) ─────────────────────
    // The backdrop catches pointer events. Clicking the backdrop = close.
    // The card itself stops propagation via on_event_stop above.
    let pd_overlay = palette_data.clone();
    container(palette_card)
        .on_click_stop(move |_| {
            pd_overlay.hide();
        })
        .debug_name("OwnStack AI Palette Overlay")
        .style(move |s| {
            s.display(if active.get() {
                Display::Flex
            } else {
                Display::None
            })
            .position(Position::Absolute)
            .z_index(1000)
            .inset(0.0)
            .size_full()
            .items_start()
            .justify_center()
            .background(crate::ownstack_theme::overlay_backdrop())
            .cursor(CursorStyle::Default)
        })
}
