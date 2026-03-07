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
        SuggestedAction {
            title: "Analyze Active File",
            keywords: &["analyze", "active", "file", "review"],
            action: SuggestedActionKind::Prompt(
                "Analyze the active file and summarize key risks and improvements.",
            ),
        },
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
                        .color(Color::from_rgb8(74, 158, 255))
                        .margin_right(8.0)
                }),
                text("AI Command Palette").style(|s| {
                    s.font_size(15.0)
                        .font_weight(Weight::BOLD)
                        .color(Color::from_rgb8(200, 225, 255))
                }),
                // Spacer
                label(|| "").style(|s| s.flex_grow(1.0)),
                label(|| "Esc").style(|s| {
                    s.font_size(10.0)
                        .padding_horiz(6.0)
                        .padding_vert(2.0)
                        .border(1.0)
                        .border_radius(4.0)
                        .border_color(Color::from_rgba8(120, 140, 180, 80))
                        .color(Color::from_rgba8(150, 170, 210, 180))
                        .margin_right(4.0)
                }),
                label(|| "Enter").style(|s| {
                    s.font_size(10.0)
                        .padding_horiz(6.0)
                        .padding_vert(2.0)
                        .border(1.0)
                        .border_radius(4.0)
                        .border_color(Color::from_rgba8(120, 140, 180, 80))
                        .color(Color::from_rgba8(150, 170, 210, 180))
                }),
            ))
            .style(|s| s.items_center().width_full()),
            // Separator line under the header
            label(|| "").style(|s| {
                s.width_full()
                    .height(1.0)
                    .margin_top(10.0)
                    .background(Color::from_rgba8(74, 158, 255, 40))
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
                        .border_color(Color::from_rgba8(74, 158, 255, 180))
                        .background(Color::from_rgb8(18, 22, 32))
                        .color(Color::WHITE)
                        .font_size(13.0)
                        .box_shadow_blur(8.0)
                        .box_shadow_color(Color::from_rgba8(74, 158, 255, 40))
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
                        .background(Color::from_rgb8(74, 158, 255))
                        .color(Color::WHITE)
                        .font_size(14.0)
                        .cursor(CursorStyle::Pointer)
                        .hover(|s| {
                            s.background(Color::from_rgb8(100, 180, 255))
                                .box_shadow_blur(12.0)
                                .box_shadow_color(Color::from_rgba8(
                                    74, 158, 255, 120,
                                ))
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
                    .color(Color::from_rgb8(120, 140, 180))
                    .padding_top(12.0)
                    .padding_bottom(6.0)
                    .border_top(1.0)
                    .border_color(Color::from_rgba8(74, 158, 255, 25))
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
                                    .color(Color::from_rgb8(74, 158, 255))
                            }),
                            text(action.title),
                        ))
                        .on_click_stop(move |_| {
                            on_click_data
                                .apply_suggested_action(action_for_click.clone());
                        })
                        .style(move |s| {
                            s.width_full()
                                .padding_horiz(10.0)
                                .padding_vert(7.0)
                                .border_radius(8.0)
                                .background(Color::from_rgba8(40, 50, 80, 120))
                                .color(Color::from_rgb8(200, 220, 255))
                                .font_size(11.5)
                                .cursor(CursorStyle::Pointer)
                                .hover(|s| {
                                    s.background(Color::from_rgba8(
                                        74, 158, 255, 100,
                                    ))
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
                    .color(Color::from_rgba8(180, 190, 220, 160))
            }),
        ))
        .style(|s| s.width_full().gap(6.0)),
        // Final Hint
        text("Tip: try '/plan' to switch agent to planning mode").style(|s| {
            s.font_size(10.0)
                .color(Color::from_rgba8(160, 180, 255, 110))
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
            .background(Color::from_rgba8(14, 18, 28, 240))
            .border(1.5)
            .border_color(Color::from_rgba8(74, 158, 255, 150))
            .border_radius(16.0)
            .box_shadow_blur(40.0)
            .box_shadow_color(Color::from_rgba8(0, 0, 0, 200))
    });

    // ── Full-screen overlay (click-outside backdrop) ─────────────────────
    // The backdrop catches pointer events. Clicking the backdrop = close.
    // The card itself stops propagation via on_event_stop above.
    let pd_overlay = palette_data.clone();
    container(palette_card)
        .on_click_stop(move |_| {
            pd_overlay.hide();
        })
        .style(move |s| {
            s.display(if active.get() {
                Display::Flex
            } else {
                Display::None
            })
            .position(Position::Absolute)
            .inset(0.0)
            .size_full()
            .items_start()
            .justify_center()
            .background(Color::from_rgba8(0, 0, 0, 80))
            .cursor(CursorStyle::Default)
        })
}
