//! Turbo Vision palette and semantic styles.
//!
//! These are deliberately the classic 16-colour values rather than a modern
//! theme: blue desktop, light gray panels, cyan selection, red hotkeys.

#![allow(dead_code)]

use ratatui::style::{Color, Modifier, Style};

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

pub const BLUE: Color = rgb(0x00, 0x00, 0xaa);
pub const BLUE_DARK: Color = rgb(0x00, 0x00, 0x55);
pub const GRAY: Color = rgb(0xaa, 0xaa, 0xaa);
pub const DARK: Color = rgb(0x55, 0x55, 0x55);
pub const CYAN: Color = rgb(0x00, 0xaa, 0xaa);
pub const CYAN_BRIGHT: Color = rgb(0x55, 0xff, 0xff);
pub const WHITE: Color = rgb(0xff, 0xff, 0xff);
pub const RED: Color = rgb(0xaa, 0x00, 0x00);
pub const YELLOW: Color = rgb(0xff, 0xff, 0x55);
pub const GREEN: Color = rgb(0x00, 0xaa, 0x00);
pub const MAGENTA: Color = rgb(0xaa, 0x00, 0xaa);
pub const BLACK: Color = rgb(0x00, 0x00, 0x00);

pub fn base() -> Style {
    Style::new().bg(BLUE).fg(GRAY)
}
pub fn body() -> Style {
    Style::new().fg(GRAY)
}
pub fn bright() -> Style {
    Style::new().fg(WHITE)
}
pub fn muted() -> Style {
    Style::new().fg(DARK)
}
pub fn faint() -> Style {
    Style::new().fg(DARK)
}

/// A tab that is not showing: a quiet label on the blue bar, not a bright
/// block competing with the active tab.
pub fn tab_idle() -> Style {
    Style::new().bg(BLUE).fg(GRAY)
}
/// A tab that is showing.
pub fn tab_active() -> Style {
    Style::new().bg(CYAN).fg(BLACK).add_modifier(Modifier::BOLD)
}
/// Pointer over an idle tab, or the `+` button.
pub fn tab_hover() -> Style {
    Style::new()
        .bg(WHITE)
        .fg(BLACK)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
}
/// Pointer over the tab that is already showing.
pub fn tab_active_hover() -> Style {
    Style::new()
        .bg(CYAN_BRIGHT)
        .fg(BLACK)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
}
/// Pointer over a tab's close button.
pub fn tab_close_hover() -> Style {
    Style::new()
        .bg(RED)
        .fg(WHITE)
        .add_modifier(Modifier::BOLD)
}
pub fn tab_number() -> Style {
    Style::new().fg(RED)
}
pub fn tab_number_on_active() -> Style {
    Style::new().fg(BLUE_DARK)
}

pub fn statusbar() -> Style {
    Style::new().bg(GRAY).fg(BLACK)
}
pub fn status_key() -> Style {
    Style::new()
        .bg(GRAY)
        .fg(RED)
        .add_modifier(Modifier::BOLD)
}
pub fn status_value() -> Style {
    Style::new().bg(GRAY).fg(BLUE_DARK).add_modifier(Modifier::BOLD)
}
pub fn status_hover() -> Style {
    Style::new()
        .bg(WHITE)
        .fg(BLACK)
        .add_modifier(Modifier::BOLD)
}
pub fn prefix_bar() -> Style {
    Style::new().bg(CYAN).fg(BLACK).add_modifier(Modifier::BOLD)
}

pub fn dialog_bg() -> Style {
    Style::new().bg(GRAY).fg(BLACK)
}
pub fn dialog_frame() -> Style {
    Style::new().bg(BLUE).fg(WHITE).add_modifier(Modifier::BOLD)
}
pub fn dialog_title() -> Style {
    Style::new().bg(GRAY).fg(BLACK).add_modifier(Modifier::BOLD)
}
pub fn dialog_shadow() -> Style {
    Style::new().bg(BLACK).fg(BLACK)
}
pub fn dialog_muted() -> Style {
    Style::new().bg(GRAY).fg(DARK)
}
pub fn list_selected() -> Style {
    Style::new().bg(CYAN).fg(BLACK).add_modifier(Modifier::BOLD)
}
pub fn list_hover() -> Style {
    Style::new().bg(WHITE).fg(BLACK)
}
pub fn list_marker() -> Style {
    Style::new().bg(GRAY).fg(RED).add_modifier(Modifier::BOLD)
}
pub fn button() -> Style {
    Style::new().bg(GRAY).fg(BLACK)
}
pub fn button_hover() -> Style {
    Style::new()
        .bg(WHITE)
        .fg(BLACK)
        .add_modifier(Modifier::BOLD)
}
pub fn button_default() -> Style {
    Style::new().bg(CYAN).fg(BLACK).add_modifier(Modifier::BOLD)
}
pub fn button_hotkey() -> Style {
    Style::new()
        .bg(GRAY)
        .fg(RED)
        .add_modifier(Modifier::BOLD)
}
pub fn selection() -> Style {
    Style::new().bg(CYAN).fg(BLACK)
}
pub fn scroll_track() -> Style {
    Style::new().bg(BLACK).fg(DARK)
}
pub fn scroll_thumb() -> Style {
    Style::new().bg(BLACK).fg(CYAN_BRIGHT)
}
pub fn live() -> Style {
    Style::new().fg(GREEN)
}
pub fn warn() -> Style {
    Style::new().fg(YELLOW)
}
pub fn error() -> Style {
    Style::new().fg(RED)
}
