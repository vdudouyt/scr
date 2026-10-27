//! Raw-terminal setup shared by attaching clients and the session picker.

use anyhow::Result;
use ratatui::crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture,
};
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::crossterm::{cursor, execute};
use std::io::{self, Write};

pub fn enter() -> Result<()> {
    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(
        out,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste,
        EnableFocusChange,
        cursor::Hide
    )?;
    // Ask for motion reports with no button held: the header's hover needs it.
    out.write_all(b"\x1b[?1003h")?;
    out.flush()?;
    Ok(())
}

pub fn leave() {
    let mut out = io::stdout();
    // Disable every mode we may have turned on, then leave the alternate
    // screen. Ignore errors: this also runs from the panic hook.
    let _ = execute!(
        out,
        DisableMouseCapture,
        DisableBracketedPaste,
        DisableFocusChange,
        LeaveAlternateScreen,
        cursor::Show
    );
    let _ = out.write_all(b"\x1b[?1003l\x1b[0m\x1b[?25h");
    let _ = out.flush();
    let _ = disable_raw_mode();
}

pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        leave();
        previous(info);
    }));
}
