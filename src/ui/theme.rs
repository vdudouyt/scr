//! Modern grayscale theme: gray text on a dark-gray canvas, with inverted
//! tab headers and borderless dialogs.

#![allow(dead_code)]

use ratatui::style::{Color, Modifier, Style};

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

// Surfaces.
pub const BG_BASE: Color = rgb(0x1c, 0x1c, 0x1c);
pub const BG_PANEL: Color = rgb(0x26, 0x26, 0x26);
pub const BG_HOVER: Color = rgb(0x30, 0x30, 0x30);
pub const BG_SEL: Color = rgb(0x3a, 0x3a, 0x3a);

// Foreground.
pub const FG: Color = rgb(0xb0, 0xb0, 0xb0);
pub const FG_BRIGHT: Color = rgb(0xf0, 0xf0, 0xf0);
pub const FG_MUTED: Color = rgb(0x80, 0x80, 0x80);
pub const FG_FAINT: Color = rgb(0x55, 0x55, 0x59);
pub const WHITE: Color = FG_BRIGHT;

pub fn base() -> Style {
    Style::new().bg(BG_BASE).fg(FG)
}
pub fn body() -> Style {
    Style::new().fg(FG)
}
pub fn bright() -> Style {
    Style::new().fg(FG_BRIGHT)
}
pub fn muted() -> Style {
    Style::new().fg(FG_MUTED)
}
pub fn faint() -> Style {
    Style::new().fg(FG_FAINT)
}

/// Idle tabs sit on the bar's own background with no block of their own. The
/// active tab is a subtle raised panel, and hover is one step brighter.
pub fn tab_idle() -> Style {
    Style::new().bg(BG_BASE).fg(FG_MUTED)
}
pub fn tab_active() -> Style {
    Style::new().bg(BG_SEL).fg(FG_BRIGHT)
}
pub fn tab_hover() -> Style {
    Style::new()
        .bg(BG_HOVER)
        .fg(FG_BRIGHT)
        .add_modifier(Modifier::UNDERLINED)
}
pub fn tab_active_hover() -> Style {
    Style::new()
        .bg(BG_HOVER)
        .fg(WHITE)
        .add_modifier(Modifier::UNDERLINED)
}
pub fn tab_close_hover() -> Style {
    Style::new()
        .bg(BG_BASE)
        .fg(FG_BRIGHT)
        .add_modifier(Modifier::BOLD)
}

pub fn key_hint() -> Style {
    Style::new().fg(FG_BRIGHT).add_modifier(Modifier::BOLD)
}

// Borderless dialogs: one filled panel, a title line, then content.
pub fn dialog_bg() -> Style {
    Style::new().bg(BG_PANEL).fg(FG)
}
pub fn dialog_title() -> Style {
    Style::new()
        .bg(BG_PANEL)
        .fg(FG_BRIGHT)
        .add_modifier(Modifier::BOLD)
}
pub fn dialog_muted() -> Style {
    Style::new().bg(BG_PANEL).fg(FG_MUTED)
}
pub fn list_selected() -> Style {
    Style::new()
        .bg(FG)
        .fg(BG_BASE)
        .add_modifier(Modifier::BOLD)
}
pub fn list_hover() -> Style {
    Style::new().bg(BG_HOVER).fg(FG_BRIGHT)
}
pub fn list_marker() -> Style {
    Style::new().bg(BG_PANEL).fg(FG_MUTED)
}
pub fn live() -> Style {
    Style::new().fg(FG)
}
pub fn warn() -> Style {
    Style::new().fg(FG_BRIGHT)
}
pub fn error() -> Style {
    Style::new().fg(FG_BRIGHT)
}
