//! Shared empty-state views used across OwnStack panels and editor areas.
//!
//! These provide informative, polished placeholders so the user never
//! sees a blank black screen. Each empty state has brand presence,
//! clear calls-to-action, and refined visual styling.

use floem::View;
use floem::peniko::Color;
use floem::style::CursorStyle;
use floem::text::Weight;
use floem::views::{Decorators, h_stack, label, v_stack};

use crate::command::LapceWorkbenchCommand;
use crate::listener::Listener;

// ── Design tokens — sourced from the central OwnStack theme ──────────────────

use crate::ownstack_theme as tok;

const TITLE_COLOR: Color = tok::TITLE;
const DESC_COLOR: Color = tok::TEXT_DIM;
const HINT_COLOR: Color = tok::TEXT_HINT;
const ICON_COLOR: Color = tok::ACCENT;
const ICON_DIM: Color = tok::ACCENT_DIM;
const CTA_BG: Color = tok::CTA_BG;
const CTA_BG_HOVER: Color = tok::CTA_BG_HOVER;
const CTA_BG_ACTIVE: Color = tok::CTA_BG_ACTIVE;
const CTA_BORDER: Color = tok::CTA_BORDER;
const CTA_BORDER_HOVER: Color = tok::CTA_BORDER_HOVER;
const CTA_TEXT: Color = tok::CTA_TEXT;
const BRAND_ACCENT: Color = tok::ACCENT;

// ── Main editor area placeholder ─────────────────────────────────────────────

/// Shown when the editor area has no active tab / no workspace.
pub fn empty_editor_placeholder(
    workbench_command: Listener<LapceWorkbenchCommand>,
) -> impl View {
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
        // CTA button — triggers the Open Folder command
        label(|| "Open Folder")
            .on_click_stop(move |_| {
                workbench_command.send(LapceWorkbenchCommand::OpenFolder);
            })
            .style(|s| {
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
                            .box_shadow_color(BRAND_ACCENT.multiply_alpha(0.14))
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

fn shortcut_tip(key: &'static str, desc: &'static str) -> impl View {
    h_stack((
        label(move || key).style(|s| {
            s.font_size(10.0)
                .font_weight(Weight::BOLD)
                .color(BRAND_ACCENT)
                .padding_horiz(6.0)
                .padding_vert(2.0)
                .background(BRAND_ACCENT.multiply_alpha(0.10))
                .border_radius(4.0)
                .selectable(false)
                .min_width(90.0)
        }),
        label(move || desc).style(|s| {
            s.font_size(11.0)
                .color(HINT_COLOR)
                .selectable(false)
        }),
    ))
    .style(|s| s.items_center().gap(8.0))
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
                .border_color(BRAND_ACCENT.multiply_alpha(0.16))
                .background(BRAND_ACCENT.multiply_alpha(0.03))
                .box_shadow_blur(16.0)
                .box_shadow_color(BRAND_ACCENT.multiply_alpha(0.08))
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
        // CTA hint
        label(|| "Type a message below and press Enter").style(|s| {
            s.padding_horiz(20.0)
                .padding_vert(9.0)
                .background(CTA_BG)
                .border(2.0)
                .border_color(CTA_BORDER)
                .border_radius(8.0)
                .color(CTA_TEXT)
                .font_size(12.0)
                .font_weight(Weight::SEMIBOLD)
                .selectable(false)
                .margin_bottom(20.0)
        }),
        // Quick-start tips
        v_stack((
            shortcut_tip("Ctrl+Shift+A", "Toggle this panel"),
            shortcut_tip("Ctrl+K", "Inline AI edit in editor"),
            shortcut_tip("Ctrl+L", "Toggle AI chat focus"),
            shortcut_tip("Ask / Plan / Auto", "Switch AI modes above"),
        ))
        .style(|s| {
            s.gap(6.0)
                .padding(12.0)
                .border(1.0)
                .border_radius(8.0)
                .border_color(BRAND_ACCENT.multiply_alpha(0.10))
                .background(BRAND_ACCENT.multiply_alpha(0.02))
                .max_width(280.0)
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
                .border_color(BRAND_ACCENT.multiply_alpha(0.12))
                .background(BRAND_ACCENT.multiply_alpha(0.02))
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
        label(|| "Place an mcp.json file in your workspace root")
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
                .color(HINT_COLOR)
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
                .color(tok::STATE_OK)
                .selectable(false)
        }),))
        .style(|s| {
            s.items_center()
                .justify_center()
                .padding(10.0)
                .margin_bottom(12.0)
                .border(1.0)
                .border_radius(10.0)
                .border_color(tok::STATE_OK.multiply_alpha(0.12))
                .background(tok::STATE_OK.multiply_alpha(0.02))
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
