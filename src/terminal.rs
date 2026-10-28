//! Raw-terminal setup shared by attaching clients and the session picker.
//!
//! All terminal modes and escape sequences come from crossterm, re-exported by
//! ratatui. Normal detach leaves the alternate screen and restores raw mode
//! through crossterm. A `SIGKILL` cannot run a handler, so each client also
//! leaves a tiny `scr-guard` process holding the read end of a pipe; if the
//! client dies without a clean leave, the pipe reaches EOF and the guardian
//! restores the terminal itself.
//!
//! Nothing here needs `unsafe`: the guardian is an ordinary child process
//! started with `Command`, its own process group keeps terminal-generated
//! signals away from it, and closing the pipe is the notification.

mod restore;

use anyhow::{Context, Result, bail};
use ratatui::crossterm::event::{EnableBracketedPaste, EnableFocusChange, EnableMouseCapture};
use ratatui::crossterm::{cursor, execute};
use std::io::{self, Read, Write};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

/// Hidden switch that makes `scr` itself act as the guardian. It is only used
/// when the sibling `scr-guard` binary is missing, for example in a `cargo run`
/// build, which does not build the helper target.
pub const GUARD_ARG: &str = "--guard";

static GUARD: Mutex<Option<io::PipeWriter>> = Mutex::new(None);
static GUARD_CHILD: Mutex<Option<Child>> = Mutex::new(None);

pub fn enter() -> Result<()> {
    // The guardian snapshots the original terminal modes before we switch to
    // raw mode, so it has to be running first.
    spawn_guardian().context("start the terminal guardian")?;
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
    let restored = restore::screen();
    // `0` means the client restored the terminal itself. If anything failed,
    // dropping the pipe makes the guardian see EOF and restore as well.
    retire_guardian(restored);
}

pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        leave();
        previous(info);
    }));
}

/// Entry point for `scr --guard`, the fallback guardian used when the sibling
/// `scr-guard` binary is not installed.
pub fn guard_main() {
    restore::guard();
}

fn spawn_guardian() -> Result<()> {
    // A guardian from an earlier enter() retires silently; we are about to own
    // the terminal again.
    retire_guardian(true);

    let (control, writer) = io::pipe().context("create the guardian pipe")?;
    let (mut ready, ready_tx) = io::pipe().context("create the guardian handshake")?;
    let exe = std::env::current_exe().context("locate the scr binary")?;
    let sibling = exe.with_file_name("scr-guard");
    let mut cmd = if sibling.is_file() {
        Command::new(sibling)
    } else {
        let mut cmd = Command::new(exe);
        cmd.arg(GUARD_ARG);
        cmd
    };
    let child = cmd
        .stdin(Stdio::from(control))
        // The handshake arrives on stderr, which the guardian does not use
        // otherwise; stdout has to stay on the terminal for restoration.
        .stderr(Stdio::from(ready_tx))
        // Its own process group: Ctrl-C, Ctrl-\ and SIGHUP are aimed at the
        // terminal's foreground group and must not kill the guardian before it
        // can restore the screen. (A terminal with TOSTOP set could stop a
        // background writer, but TOSTOP is off by default.)
        .process_group(0)
        .spawn()
        .context("start the guardian process")?;

    // Wait until the guardian has snapshotted the original terminal modes
    // before entering raw mode. EOF here means the guardian died immediately.
    let mut started = [0u8; 1];
    ready
        .read_exact(&mut started)
        .context("the terminal guardian did not start")?;
    if started != *b"r" {
        bail!("the terminal guardian did not start");
    }

    *GUARD.lock().unwrap_or_else(|e| e.into_inner()) = Some(writer);
    *GUARD_CHILD.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
    Ok(())
}

/// Tell the guardian how this process is leaving, then reap it.
///
/// `clean` means the terminal was restored here and the guardian can exit
/// without touching it; otherwise closing the pipe is the request to restore.
fn retire_guardian(clean: bool) {
    let writer = GUARD.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(mut writer) = writer {
        if clean {
            let _ = writer.write_all(b"0");
        }
        // Dropping the writer closes the pipe, so the guardian sees EOF.
    }
    let child = GUARD_CHILD
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    if let Some(mut child) = child {
        // Reap off the main thread: a stalled terminal must not keep the
        // client from exiting.
        let _ = std::thread::Builder::new()
            .name("scr-guard-reaper".into())
            .spawn(move || {
                let _ = child.wait();
            });
    }
}
