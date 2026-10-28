//! Terminal restoration and the guardian side of its pipe protocol.
//!
//! This file is compiled twice: as `terminal::restore` in the client and,
//! through `#[path]`, as a module of `src/bin/scr-guard.rs`. Keep it free of
//! crate-relative imports.
//!
//! crossterm remembers the termios it replaced in a process-local static, so
//! a guardian in a separate process cannot ask it to undo raw mode. The
//! guardian therefore snapshots the original modes with `rustix` before the
//! client enters raw mode, and applies them itself if the client dies without
//! restoring them.

use ratatui::crossterm::event::{DisableBracketedPaste, DisableFocusChange, DisableMouseCapture};
use ratatui::crossterm::style::{Attribute, ResetColor, SetAttribute};
use ratatui::crossterm::{cursor, execute};
use rustix::termios::{OptionalActions, Termios, tcgetattr, tcsetattr};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsFd;

/// Undo the screen modes set by `terminal::enter`.
///
/// In the client `try_restore` also restores termios from crossterm's static;
/// in the guardian that part is a no-op and [`guard`] handles termios.
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

fn tty() -> Option<(File, Termios)> {
    // stdout is the terminal in normal use. Fall back to /dev/tty when it is
    // redirected. Either descriptor is dup'ed or kept open for the guardian's
    // whole life: once the client (the session leader) dies, /dev/tty can no
    // longer be opened, but an open terminal descriptor still works.
    let stdout = io::stdout()
        .as_fd()
        .try_clone_to_owned()
        .ok()
        .map(File::from);
    let dev_tty = || OpenOptions::new().read(true).write(true).open("/dev/tty").ok();
    stdout
        .into_iter()
        .chain(dev_tty())
        .find_map(|file| tcgetattr(&file).ok().map(|modes| (file, modes)))
}

/// Guardian entry point.
///
/// The client holds the write end of our stdin pipe. First we snapshot the
/// original terminal modes and answer on stderr so the client may safely enter
/// raw mode. Then a `0` byte means it restored everything itself, while EOF
/// (including after SIGKILL) means we have to restore the terminal.
pub fn guard() {
    let tty = tty();
    let _ = io::stderr().write_all(b"r");
    let _ = io::stderr().flush();

    let mut byte = [0u8; 1];
    loop {
        match io::stdin().read(&mut byte) {
            Ok(1) if byte[0] == b'0' => return,
            Ok(_) => break,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    if let Some((tty, saved)) = tty.as_ref() {
        let _ = tcsetattr(tty, OptionalActions::Now, saved);
    }
    screen();
}
