//! Raw-terminal setup shared by attaching clients and the session picker.
//!
//! All terminal modes and escape sequences come from crossterm, re-exported by
//! ratatui. Normal detach leaves the alternate screen and restores raw mode
//! through crossterm. A `SIGKILL` cannot run a handler, so each client also
//! leaves a tiny `scr-guard` process holding a pipe; if the client dies without
//! a clean leave, the guardian runs the same crossterm restore itself.

use anyhow::Result;
use ratatui::crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture,
};
use ratatui::crossterm::style::{Attribute, ResetColor, SetAttribute};
use ratatui::crossterm::{cursor, execute};
use std::io::{self, Write};
use std::sync::atomic::{AtomicI32, Ordering};

static GUARD_FD: AtomicI32 = AtomicI32::new(-1);

pub fn enter() -> Result<()> {
    install_signal_handlers();
    // The same setup OpenAdmin uses: raw mode plus the alternate screen,
    // with ratatui's standard panic hook. Mouse, paste and focus capture are
    // separate crossterm commands, also as in OpenAdmin.
    if let Err(e) = ratatui::try_init() {
        let _ = ratatui::try_restore();
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
        let _ = restore_terminal();
        return Err(e.into());
    }
    if !spawn_guardian() {
        leave();
        return Err(anyhow::anyhow!("could not start the terminal guardian"));
    }
    Ok(())
}

pub fn leave() {
    let restored = restore_terminal();
    // `0` means the client restored the terminal itself. `R` asks the
    // guardian to do it; EOF (SIGKILL) means the same thing.
    notify_guardian(if restored { b'0' } else { b'R' });
}

pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        leave();
        previous(info);
    }));
}

fn restore_terminal() -> bool {
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

fn notify_guardian(byte: u8) {
    let fd = GUARD_FD.swap(-1, Ordering::SeqCst);
    if fd >= 0 {
        unsafe {
            let _ = libc::write(fd, &byte as *const u8 as *const libc::c_void, 1);
            libc::close(fd);
        }
    }
}

fn install_signal_handlers() {
    unsafe {
        for sig in [
            libc::SIGTERM,
            libc::SIGHUP,
            libc::SIGINT,
            libc::SIGQUIT,
        ] {
            libc::signal(
                sig,
                handle_signal as *const () as libc::sighandler_t,
            );
        }
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
}

extern "C" fn handle_signal(sig: libc::c_int) {
    // The signal handler does no terminal work: it hands restoration to the
    // guardian (normal Rust code, not an async signal context) and exits.
    unsafe {
        let fd = GUARD_FD.swap(-1, Ordering::SeqCst);
        if fd >= 0 {
            let _ = libc::write(fd, b"R".as_ptr() as *const libc::c_void, 1);
            libc::close(fd);
        }
        libc::_exit(128 + sig);
    }
}

/// Fork the small process that owns restoration after SIGKILL.
///
/// It is renamed to `scr-guard` so `killall scr` does not take it down with
/// the client and daemons. A clean leave sends `0`; the guardian then exits
/// without touching the terminal.
fn spawn_guardian() -> bool {
    unsafe {
        let mut fds = [0i32; 2];
        if libc::pipe(fds.as_mut_ptr()) != 0 {
            return false;
        }
        let pid = libc::fork();
        if pid < 0 {
            libc::close(fds[0]);
            libc::close(fds[1]);
            return false;
        }
        if pid == 0 {
            libc::close(fds[1]);
            let name = b"scr-guard\0";
            let _ = libc::prctl(
                libc::PR_SET_NAME,
                name.as_ptr() as libc::c_ulong,
                0,
                0,
                0,
            );
            for sig in [
                libc::SIGHUP,
                libc::SIGINT,
                libc::SIGTERM,
                libc::SIGQUIT,
                libc::SIGPIPE,
            ] {
                libc::signal(sig, libc::SIG_IGN);
            }
            let mut byte = 0u8;
            loop {
                let n = libc::read(
                    fds[0],
                    &mut byte as *mut u8 as *mut libc::c_void,
                    1,
                );
                if n == 1 {
                    if byte == b'0' {
                        libc::_exit(0);
                    }
                    break;
                }
                if n == 0 {
                    break;
                }
                if std::io::Error::last_os_error().kind()
                    != std::io::ErrorKind::Interrupted
                {
                    break;
                }
            }
            let _ = restore_terminal();
            libc::_exit(0);
        }
        libc::close(fds[0]);
        let old = GUARD_FD.swap(fds[1], Ordering::SeqCst);
        if old >= 0 {
            let _ = libc::write(old, b"0".as_ptr() as *const libc::c_void, 1);
            libc::close(old);
        }
        true
    }
}
