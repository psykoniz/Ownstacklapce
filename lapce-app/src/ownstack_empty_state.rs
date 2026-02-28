//! Shared empty-state views used across OwnStack panels and editor areas.
//!
//! These provide informative, minimal placeholders so the user never
//! sees a blank black screen.

use floem::peniko::Color;
use floem::style::CursorStyle;
use floem::text::Weight;
use floem::views::{label, v_stack, Decorators};
use floem::View;

// ── Design tokens (consistent across all empty states) ───────────────────────

const TITLE_COLOR: Color = Color::from_rgb8(180, 200, 230);
const DESC_COLOR: Color = Color::from_rgb8(120, 140, 170);
const ICON_COLOR: Color = Color::from_rgb8(74, 130, 200);
const CTA_BG: Color = Color::from_rgb8(30, 45, 70);
const CTA_BORDER: Color = Color::from_rgb8(60, 90, 140);

// ── Main editor area placeholder ─────────────────────────────────────────────

/// Shown when the editor area has no active tab / no workspace.
pub fn empty_editor_placeholder() -> impl View {
    v_stack((
        label(|| "{ }").style(|s| {
            s.font_size(36.0)
                .color(ICON_COLOR)
                .margin_bottom(16.0)
                .font_weight(Weight::BOLD)
        }),
        label(|| "Open a folder to start").style(|s| {
            s.font_size(16.0)
                .font_weight(Weight::SEMIBOLD)
                .color(TITLE_COLOR)
                .margin_bottom(8.0)
        }),
        label(|| "Choose a workspace to browse files, use AI chat, and run commands.").style(|s| {
            s.font_size(12.0)
                .color(DESC_COLOR)
                .margin_bottom(20.0)
                .max_width(360.0)
                .line_height(1.5)
        }),
        label(|| "Open Folder")
            .style(|s| {
                s.padding_horiz(20.0)
                    .padding_vert(10.0)
                    .background(CTA_BG)
                    .border(1.0)
                    .border_color(CTA_BORDER)
                    .border_radius(6.0)
                    .color(Color::from_rgb8(180, 210, 255))
                    .font_size(13.0)
                    .cursor(CursorStyle::Pointer)
                    .hover(|s| {
                        s.background(Color::from_rgb8(40, 60, 90))
                            .border_color(Color::from_rgb8(80, 120, 180))
                    })
            }),
    ))
    .style(|s| {
        s.size_full()
            .items_center()
            .justify_center()
            .flex_col()
    })
}

// ── Chat panel empty state ───────────────────────────────────────────────────

/// Shown in the AI Chat sidebar when no messages exist yet.
pub fn chat_empty_state() -> impl View {
    v_stack((
        label(|| "AI").style(|s| {
            s.font_size(28.0)
                .color(ICON_COLOR)
                .margin_bottom(12.0)
                .font_weight(Weight::BOLD)
        }),
        label(|| "No conversation yet").style(|s| {
            s.font_size(14.0)
                .font_weight(Weight::SEMIBOLD)
                .color(TITLE_COLOR)
                .margin_bottom(6.0)
        }),
        label(|| "Type a message below to start a conversation with the AI agent.").style(|s| {
            s.font_size(11.0)
                .color(DESC_COLOR)
                .max_width(240.0)
                .line_height(1.5)
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
        label(|| "MCP").style(|s| {
            s.font_size(28.0)
                .color(ICON_COLOR)
                .margin_bottom(12.0)
                .font_weight(Weight::BOLD)
        }),
        label(|| "No MCP servers configured").style(|s| {
            s.font_size(14.0)
                .font_weight(Weight::SEMIBOLD)
                .color(TITLE_COLOR)
                .margin_bottom(6.0)
        }),
        label(|| "Add a server to connect AI agents to external tools and data sources.").style(|s| {
            s.font_size(11.0)
                .color(DESC_COLOR)
                .max_width(260.0)
                .line_height(1.5)
                .margin_bottom(12.0)
        }),
        label(|| "Add MCP Server")
            .style(|s| {
                s.padding_horiz(16.0)
                    .padding_vert(8.0)
                    .background(CTA_BG)
                    .border(1.0)
                    .border_color(CTA_BORDER)
                    .border_radius(6.0)
                    .color(Color::from_rgb8(180, 210, 255))
                    .font_size(12.0)
                    .cursor(CursorStyle::Pointer)
                    .hover(|s| {
                        s.background(Color::from_rgb8(40, 60, 90))
                            .border_color(Color::from_rgb8(80, 120, 180))
                    })
                    .margin_bottom(14.0)
            }),
        label(move || format!("Searched: {}", if searched_paths.is_empty() { "(no workspace)" } else { &searched_paths })).style(|s| {
            s.font_size(9.5)
                .color(Color::from_rgb8(80, 100, 130))
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

// ── Audit panel empty state ──────────────────────────────────────────────────

/// Shown in the Audit panel when 0 entries.
pub fn audit_empty_state() -> impl View {
    v_stack((
        label(|| "No audit entries yet").style(|s| {
            s.font_size(13.0)
                .font_weight(Weight::SEMIBOLD)
                .color(TITLE_COLOR)
                .margin_bottom(6.0)
        }),
        label(|| "Actions will appear here when you run AI commands.").style(|s| {
            s.font_size(11.0)
                .color(DESC_COLOR)
                .max_width(280.0)
                .line_height(1.5)
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
