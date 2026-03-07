//! Shared empty-state views used across OwnStack panels and editor areas.
//!
//! These provide informative, polished placeholders so the user never
//! sees a blank black screen. Each empty state has brand presence,
//! clear calls-to-action, and refined visual styling.

use floem::View;
use floem::peniko::Color;
use floem::style::CursorStyle;
use floem::text::Weight;
use floem::views::{Decorators, label, v_stack};

// ── Design tokens (consistent across all empty states) ───────────────────────

const TITLE_COLOR: Color = Color::from_rgb8(190, 210, 235);
const DESC_COLOR: Color = Color::from_rgb8(130, 150, 180);
const HINT_COLOR: Color = Color::from_rgb8(95, 115, 145);
const ICON_COLOR: Color = Color::from_rgb8(74, 158, 255);
const ICON_DIM: Color = Color::from_rgb8(55, 100, 170);
const CTA_BG: Color = Color::from_rgb8(28, 48, 78);
const CTA_BG_HOVER: Color = Color::from_rgb8(38, 65, 105);
const CTA_BG_ACTIVE: Color = Color::from_rgb8(32, 55, 88);
const CTA_BORDER: Color = Color::from_rgb8(60, 100, 160);
const CTA_BORDER_HOVER: Color = Color::from_rgb8(80, 130, 200);
const CTA_TEXT: Color = Color::from_rgb8(180, 215, 255);
const BRAND_ACCENT: Color = Color::from_rgb8(74, 158, 255);

// ── Main editor area placeholder ─────────────────────────────────────────────

/// Shown when the editor area has no active tab / no workspace.
pub fn empty_editor_placeholder() -> impl View {
    v_stack((
        // Brand diamond icon
        label(|| "\u{27D0}")
            .style(|s| s.font_size(48.0).color(BRAND_ACCENT).margin_bottom(4.0)),
        // Brand name
        label(|| "OwnStack").style(|s| {
            s.font_size(26.0)
                .font_weight(Weight::BOLD)
                .color(TITLE_COLOR)
                .margin_bottom(4.0)
                .selectable(false)
        }),
        // Tagline
        label(|| "AI-native code editor").style(|s| {
            s.font_size(12.0)
                .color(HINT_COLOR)
                .margin_bottom(24.0)
                .selectable(false)
        }),
        // Description
        label(
            || "Open a workspace to browse files, chat with AI, and run commands.",
        )
        .style(|s| {
            s.font_size(13.0)
                .color(DESC_COLOR)
                .margin_bottom(24.0)
                .max_width(400.0)
                .line_height(1.6)
                .selectable(false)
        }),
        // CTA button
        label(|| "Open Folder").style(|s| {
            s.padding_horiz(24.0)
                .padding_vert(10.0)
                .background(CTA_BG)
                .border(2.0)
                .border_color(CTA_BORDER)
                .border_radius(8.0)
                .color(CTA_TEXT)
                .font_size(13.0)
                .font_weight(Weight::SEMIBOLD)
                .cursor(CursorStyle::Pointer)
                .hover(|s| {
                    s.background(CTA_BG_HOVER)
                        .border_color(CTA_BORDER_HOVER)
                        .box_shadow_blur(8.0)
                        .box_shadow_color(Color::from_rgba8(74, 158, 255, 35))
                })
                .active(|s| s.background(CTA_BG_ACTIVE).border_color(CTA_BORDER))
                .margin_bottom(16.0)
        }),
        // Keyboard shortcut hint
        label(|| "Ctrl+O to open a folder")
            .style(|s| s.font_size(11.0).color(HINT_COLOR).selectable(false)),
    ))
    .style(|s| s.size_full().items_center().justify_center().flex_col())
}

// ── Chat panel empty state ───────────────────────────────────────────────────

/// Shown in the AI Chat sidebar when no messages exist yet.
pub fn chat_empty_state() -> impl View {
    v_stack((
        // Chat bubble icon area with subtle glow border
        v_stack((
            label(|| "\u{25CB}  \u{25CF}  \u{25CB}").style(|s| {
                s.font_size(14.0)
                    .color(ICON_DIM)
                    .margin_bottom(6.0)
                    .selectable(false)
            }),
            label(|| "\u{27D0} AI").style(|s| {
                s.font_size(22.0)
                    .font_weight(Weight::BOLD)
                    .color(BRAND_ACCENT)
                    .selectable(false)
            }),
        ))
        .style(|s| {
            s.items_center()
                .justify_center()
                .padding_horiz(28.0)
                .padding_vert(16.0)
                .margin_bottom(16.0)
                .border(1.0)
                .border_radius(12.0)
                .border_color(Color::from_rgba8(74, 158, 255, 40))
                .background(Color::from_rgba8(74, 158, 255, 8))
                .box_shadow_blur(16.0)
                .box_shadow_color(Color::from_rgba8(74, 158, 255, 20))
        }),
        // Title
        label(|| "Start a conversation").style(|s| {
            s.font_size(15.0)
                .font_weight(Weight::SEMIBOLD)
                .color(TITLE_COLOR)
                .margin_bottom(8.0)
                .selectable(false)
        }),
        // Description
        label(|| "Ask a question, paste code for review, or describe what you want to build.")
            .style(|s| {
                s.font_size(12.0)
                    .color(DESC_COLOR)
                    .max_width(320.0)
                    .line_height(1.6)
                    .margin_bottom(16.0)
                    .selectable(false)
            }),
        // Hint
        label(|| "Start by asking a question or pasting code").style(|s| {
            s.font_size(11.0)
                .color(HINT_COLOR)
                .selectable(false)
        }),
    ))
    .style(|s| {
        s.width_full()
            .flex_grow(1.0)
            .items_center()
            .justify_center()
            .flex_col()
            .padding(20.0)
    })
}

// ── MCP panel empty state ────────────────────────────────────────────────────

/// Shown in the MCP panel when no servers are configured.
pub fn mcp_empty_state(searched_paths: String) -> impl View {
    v_stack((
        // MCP icon area
        v_stack((
            label(|| "\u{29BF}").style(|s| {
                s.font_size(28.0)
                    .color(ICON_COLOR)
                    .selectable(false)
            }),
        ))
        .style(|s| {
            s.items_center()
                .justify_center()
                .padding(12.0)
                .margin_bottom(12.0)
                .border(1.0)
                .border_radius(10.0)
                .border_color(Color::from_rgba8(74, 158, 255, 30))
                .background(Color::from_rgba8(74, 158, 255, 6))
        }),
        label(|| "No MCP servers configured").style(|s| {
            s.font_size(14.0)
                .font_weight(Weight::SEMIBOLD)
                .color(TITLE_COLOR)
                .margin_bottom(8.0)
                .selectable(false)
        }),
        label(|| "Add a server to connect AI agents to external tools and data sources.")
            .style(|s| {
                s.font_size(12.0)
                    .color(DESC_COLOR)
                    .max_width(300.0)
                    .line_height(1.6)
                    .margin_bottom(16.0)
                    .selectable(false)
            }),
        label(|| "Add MCP Server")
            .style(|s| {
                s.padding_horiz(20.0)
                    .padding_vert(9.0)
                    .background(CTA_BG)
                    .border(2.0)
                    .border_color(CTA_BORDER)
                    .border_radius(8.0)
                    .color(CTA_TEXT)
                    .font_size(12.0)
                    .font_weight(Weight::SEMIBOLD)
                    .cursor(CursorStyle::Pointer)
                    .hover(|s| {
                        s.background(CTA_BG_HOVER)
                            .border_color(CTA_BORDER_HOVER)
                            .box_shadow_blur(8.0)
                            .box_shadow_color(Color::from_rgba8(74, 158, 255, 35))
                    })
                    .active(|s| {
                        s.background(CTA_BG_ACTIVE)
                            .border_color(CTA_BORDER)
                    })
                    .margin_bottom(14.0)
            }),
        label(move || {
            format!(
                "Searched: {}",
                if searched_paths.is_empty() {
                    "(no workspace)"
                } else {
                    &searched_paths
                }
            )
        })
        .style(|s| {
            s.font_size(10.0)
                .color(Color::from_rgb8(80, 100, 130))
                .max_width(300.0)
                .selectable(false)
        }),
    ))
    .style(|s| {
        s.width_full()
            .flex_grow(1.0)
            .items_center()
            .justify_center()
            .flex_col()
            .padding(20.0)
    })
}

// ── Audit panel empty state ──────────────────────────────────────────────────

/// Shown in the Audit panel when 0 entries.
pub fn audit_empty_state() -> impl View {
    v_stack((
        // Shield icon for security audit theming
        v_stack((label(|| "\u{26E8}").style(|s| {
            s.font_size(26.0)
                .color(Color::from_rgb8(100, 200, 150))
                .selectable(false)
        }),))
        .style(|s| {
            s.items_center()
                .justify_center()
                .padding(10.0)
                .margin_bottom(12.0)
                .border(1.0)
                .border_radius(10.0)
                .border_color(Color::from_rgba8(100, 200, 150, 30))
                .background(Color::from_rgba8(100, 200, 150, 6))
        }),
        label(|| "No audit entries yet").style(|s| {
            s.font_size(14.0)
                .font_weight(Weight::SEMIBOLD)
                .color(TITLE_COLOR)
                .margin_bottom(8.0)
                .selectable(false)
        }),
        label(|| "All AI actions and tool calls will be recorded here for review.")
            .style(|s| {
                s.font_size(12.0)
                    .color(DESC_COLOR)
                    .max_width(300.0)
                    .line_height(1.6)
                    .selectable(false)
            }),
    ))
    .style(|s| {
        s.width_full()
            .items_center()
            .justify_center()
            .flex_col()
            .padding_vert(40.0)
    })
}
