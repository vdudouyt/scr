//! Terminal screen restoration.
//!
//! Keep this free of crate-relative imports.

use ratatui::crossterm::event::{DisableBracketedPaste, DisableFocusChange, DisableMouseCapture};
use ratatui::crossterm::style::{Attribute, ResetColor, SetAttribute};
use ratatui::crossterm::{cursor, execute};
use std::io::{self, Write};

/// Undo the screen modes set by `terminal::enter`.
pub fn screen() -> bool {
    let mut out = io::stdout();
    let commands = execute!(
        out,
        DisableMouseCapture,
        DisableBracketedPaste,
        DisableFocusChange,
        ResetColor,
        SetAttribute(Attribute::Reset),
        cursor::Show
    )
    .is_ok();
    let _ = out.flush();
    let screen = ratatui::try_restore().is_ok();
    commands && screen
}
