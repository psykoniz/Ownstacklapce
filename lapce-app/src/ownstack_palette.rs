use floem::prelude::{SignalGet, SignalUpdate};
use floem::reactive::{RwSignal, create_rw_signal};
use lapce_rpc::ownstack::OwnStackRpc;

use crate::window_tab::CommonData;

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

        // Send AI prompt via RPC
        let message = OwnStackRpc::AiPrompt { prompt };
        self.common.proxy.ownstack(message);
        tracing::info!("OwnStack Palette: AiPrompt sent");

        self.hide();
    }
}

/// AI Command Palette bar — shown inline at top of panel when active.
/// Toggled via `palette_data.active`. Place it at the top of the right panel
/// or whichever layout host is appropriate.
pub fn ownstack_palette_view(palette_data: OwnStackPaletteData) -> impl floem::View {
    use floem::peniko::Color;
    use floem::prelude::SignalGet;
    use floem::style::{CursorStyle, Display, Position};
    use floem::views::{Decorators, h_stack, label, text, text_input, v_stack};

    let active = palette_data.active;
    let input = palette_data.input;

    v_stack((
        // Row 1: title + hint
        h_stack((
            text("⚡ OwnStack — AI Command")
                .style(|s| s.font_size(13.0).color(Color::from_rgb8(180, 220, 255))),
            label(|| "Esc: close  Enter: send").style(|s| {
                s.font_size(10.0)
                    .color(Color::from_rgba8(180, 200, 255, 160))
            }),
        ))
        .style(|s| s.justify_between().items_center().padding_bottom(8.0)),
        // Row 2: input + send button
        h_stack((
            text_input(input)
                .placeholder("Ask AI anything…")
                .style(|s| {
                    s.width_full()
                        .padding(9.0)
                        .border(1.5)
                        .border_radius(8.0)
                        .border_color(Color::from_rgb8(74, 158, 255))
                        .background(Color::from_rgb8(18, 22, 32))
                        .color(Color::WHITE)
                        .font_size(13.0)
                        .box_shadow_blur(6.0)
                        .box_shadow_color(Color::from_rgba8(74, 158, 255, 50))
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
                    s.padding_horiz(14.0)
                        .padding_vert(9.0)
                        .border_radius(8.0)
                        .background(Color::from_rgb8(74, 158, 255))
                        .color(Color::WHITE)
                        .font_size(14.0)
                        .cursor(CursorStyle::Pointer)
                        .hover(|s| s.background(Color::from_rgb8(100, 180, 255)))
                })
                .on_click_stop({
                    let pd = palette_data.clone();
                    move |_| pd.submit()
                }),
        ))
        .style(|s| s.width_full().items_center().gap(10.0)),
        // Row 3: hint
        text("Tip: prefix with /plan, /auto or /ask to set mode").style(|s| {
            s.font_size(10.0)
                .color(Color::from_rgba8(160, 180, 255, 130))
                .padding_top(6.0)
        }),
    ))
    .style(move |s| {
        s.display(if active.get() {
            Display::Flex
        } else {
            Display::None
        })
        .position(Position::Absolute)
        .margin_top(48.0)
        .margin_horiz(16.0)
        .max_width(720.0)
        .width_pct(100.0)
        .padding(14.0)
        .background(Color::from_rgb8(14, 18, 28))
        .border(1.5)
        .border_color(Color::from_rgb8(74, 158, 255))
        .border_radius(10.0)
        .box_shadow_blur(18.0)
        .box_shadow_color(Color::from_rgba8(74, 158, 255, 70))
    })
}
