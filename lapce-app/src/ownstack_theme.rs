//! OwnStack design tokens — a single source of truth for the AI surfaces.
//!
//! The base Lapce theme drives the editor chrome; these tokens give the
//! OwnStack AI surfaces (chat, palette, empty states, audit) a cohesive,
//! modern look without threading theme TOML through every component. Use
//! these instead of ad-hoc `Color::from_rgb8(...)` literals so the whole
//! AI experience restyles from one place.

use floem::peniko::Color;

// ── Brand ────────────────────────────────────────────────────────────────────

/// Primary brand accent (links, focus, primary actions).
pub const ACCENT: Color = Color::from_rgb8(74, 158, 255);
/// Brighter accent for hover/active emphasis.
pub const ACCENT_BRIGHT: Color = Color::from_rgb8(120, 185, 255);
/// Dim accent for subtle borders/glows.
pub const ACCENT_DIM: Color = Color::from_rgb8(55, 100, 170);

// ── Surfaces (dark, layered) ─────────────────────────────────────────────────

/// Deepest surface (overlay backdrops, palette card base).
pub const SURFACE_0: Color = Color::from_rgb8(14, 18, 28);
/// Raised surface (cards, inputs).
pub const SURFACE_1: Color = Color::from_rgb8(18, 22, 32);
/// Interactive surface (chips, list rows).
pub const SURFACE_2: Color = Color::from_rgb8(28, 34, 48);
/// Hovered interactive surface.
pub const SURFACE_HOVER: Color = Color::from_rgb8(38, 48, 70);

// ── Borders ──────────────────────────────────────────────────────────────────

pub const BORDER: Color = Color::from_rgba8(120, 140, 180, 70);
pub const BORDER_STRONG: Color = Color::from_rgba8(74, 158, 255, 150);

// ── Text hierarchy ───────────────────────────────────────────────────────────

/// Primary text on dark surfaces.
pub const TEXT: Color = Color::from_rgb8(200, 220, 255);
/// Headings / emphasized titles.
pub const TITLE: Color = Color::from_rgb8(190, 210, 235);
/// Secondary/descriptive text.
pub const TEXT_DIM: Color = Color::from_rgb8(130, 150, 180);
/// Tertiary hints / placeholders.
pub const TEXT_HINT: Color = Color::from_rgb8(95, 115, 145);

// ── Call-to-action ───────────────────────────────────────────────────────────

pub const CTA_BG: Color = Color::from_rgb8(28, 48, 78);
pub const CTA_BG_HOVER: Color = Color::from_rgb8(38, 65, 105);
pub const CTA_BG_ACTIVE: Color = Color::from_rgb8(32, 55, 88);
pub const CTA_BORDER: Color = Color::from_rgb8(60, 100, 160);
pub const CTA_BORDER_HOVER: Color = Color::from_rgb8(80, 130, 200);
pub const CTA_TEXT: Color = Color::from_rgb8(180, 215, 255);

// ── Agent state ──────────────────────────────────────────────────────────────

pub const STATE_RUNNING: Color = Color::from_rgb8(74, 158, 255);
pub const STATE_IDLE: Color = Color::from_rgb8(120, 140, 170);
pub const STATE_OK: Color = Color::from_rgb8(100, 200, 150);
pub const STATE_WARN: Color = Color::from_rgb8(230, 180, 80);
pub const STATE_ERROR: Color = Color::from_rgb8(235, 110, 110);

// ── Spacing scale (px) ───────────────────────────────────────────────────────

pub const SPACE_XS: f64 = 4.0;
pub const SPACE_SM: f64 = 8.0;
pub const SPACE_MD: f64 = 12.0;
pub const SPACE_LG: f64 = 16.0;
pub const SPACE_XL: f64 = 24.0;

// ── Radii (px) ───────────────────────────────────────────────────────────────

pub const RADIUS_SM: f64 = 6.0;
pub const RADIUS_MD: f64 = 10.0;
pub const RADIUS_LG: f64 = 16.0;
/// Pill / fully-rounded.
pub const RADIUS_PILL: f64 = 999.0;

/// A translucent backdrop for full-screen overlays.
pub fn overlay_backdrop() -> Color {
    Color::from_rgba8(0, 0, 0, 110)
}
