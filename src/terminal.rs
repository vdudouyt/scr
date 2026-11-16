//! Raw-terminal setup shared by attaching clients and the session picker.
//!
//! All terminal modes and escape sequences come from crossterm, re-exported by
//! ratatui. Normal detach leaves the alternate screen and restores raw mode
//! through crossterm.
//!
//! Nothing here needs `unsafe`.

mod restore;

use anyhow::Result;
use ratatui::crossterm::event::{EnableBracketedPaste, EnableFocusChange, EnableMouseCapture};
use ratatui::crossterm::{cursor, execute};
use std::io;

pub fn enter() -> Result<()> {
    // Raw mode plus the alternate screen, with ratatui's standard panic hook.
    // Mouse, paste and focus capture are separate crossterm commands, also as
    // in OpenAdmin.
    if let Err(e) = ratatui::try_init() {
        leave();
        return Err(e.into());
    }
    let mut out = io::stdout();
    if let Err(e) = execute!(
        out,
        EnableMouseCapture,
        EnableBracketedPaste,
        EnableFocusChange,
        cursor::Hide
    ) {
        leave();
        return Err(e.into());
    }
    Ok(())
}

pub fn leave() {
    restore::screen();
}

pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        leave();
        previous(info);
    }));
}
