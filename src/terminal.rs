//! Raw-terminal setup shared by attaching clients and the session picker.
//!
//! Detaching restores the terminal through the normal path. A `SIGKILL` cannot
//! run a handler, so every client also leaves a tiny guardian process holding
//! the original termios and a pipe. If the client dies without calling
//! `leave`, the guardian restores the terminal itself.

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
use std::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

const RESTORE: &[u8] = b"\x1b[?1003l\x1b[?1002l\x1b[?1000l\x1b[?1015l\x1b[?1006l\x1b[?2004l\x1b[?1004l\x1b[?1049l\x1b[0m\x1b[?25h";

static GUARD_FD: AtomicI32 = AtomicI32::new(-1);
static ORIG_TERMIOS: AtomicPtr<libc::termios> = AtomicPtr::new(std::ptr::null_mut());

pub fn enter() -> Result<()> {
    let original = remember_termios();
    install_signal_handlers();

    enable_raw_mode()?;
    let mut out = io::stdout();
    if let Err(e) = execute!(
        out,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste,
        EnableFocusChange,
        cursor::Hide
    ) {
        let _ = disable_raw_mode();
        return Err(e.into());
    }
    if out.write_all(b"\x1b[?1003h").and_then(|_| out.flush()).is_err() {
        let _ = disable_raw_mode();
        return Err(anyhow::anyhow!("enable all-motion mouse reporting"));
    }
    spawn_guardian(original);
    Ok(())
}

pub fn leave() {
    let mut out = io::stdout();
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

    notify_guardian();
}

fn notify_guardian() {
    let fd = GUARD_FD.swap(-1, Ordering::SeqCst);
    if fd >= 0 {
        unsafe {
            let _ = libc::write(fd, b"0".as_ptr() as *const libc::c_void, 1);
            libc::close(fd);
        }
    }
}

pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        leave();
        previous(info);
    }));
}

fn remember_termios() -> Option<libc::termios> {
    unsafe {
        let mut term: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(0, &mut term) != 0 {
            return None;
        }
        let copy = term;
        let ptr = Box::into_raw(Box::new(term));
        // A second enter (picker, then attach) replaces the saved state.
        let old = ORIG_TERMIOS.swap(ptr, Ordering::SeqCst);
        if !old.is_null() {
            drop(Box::from_raw(old));
        }
        Some(copy)
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
            libc::signal(sig, handle_signal as *const () as libc::sighandler_t);
        }
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
}

extern "C" fn handle_signal(sig: libc::c_int) {
    notify_guardian();
    unsafe {
        restore_now();
        libc::_exit(128 + sig);
    }
}

unsafe fn restore_now() {
    unsafe {
        let _ = libc::write(
            1,
            RESTORE.as_ptr() as *const libc::c_void,
            RESTORE.len(),
        );
        let ptr = ORIG_TERMIOS.load(Ordering::SeqCst);
        if !ptr.is_null() {
            let _ = libc::tcsetattr(0, libc::TCSANOW, ptr);
        }
    }
}

/// Fork the small process that owns the terminal restore path for SIGKILL.
///
/// It is renamed to `scr-guard` so `killall scr` does not take it down with
/// the client and daemon. A normal `leave` sends one byte; the guardian then
/// exits without touching the terminal.
fn spawn_guardian(original: Option<libc::termios>) {
    unsafe {
        let mut fds = [0i32; 2];
        if libc::pipe(fds.as_mut_ptr()) != 0 {
            return;
        }
        let pid = libc::fork();
        if pid < 0 {
            libc::close(fds[0]);
            libc::close(fds[1]);
            return;
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
            loop {
                let mut byte = 0u8;
                let n = libc::read(
                    fds[0],
                    &mut byte as *mut u8 as *mut libc::c_void,
                    1,
                );
                if n == 1 {
                    libc::_exit(0);
                }
                if n == 0 {
                    break;
                }
                if *libc::__errno_location() != libc::EINTR {
                    break;
                }
            }
            // Parent died without a clean leave.
            let _ = libc::write(
                1,
                RESTORE.as_ptr() as *const libc::c_void,
                RESTORE.len(),
            );
            if let Some(term) = &original {
                let _ = libc::tcsetattr(0, libc::TCSANOW, term);
            }
            libc::_exit(0);
        }
        libc::close(fds[0]);
        let old = GUARD_FD.swap(fds[1], Ordering::SeqCst);
        if old >= 0 {
            let _ = libc::write(old, b"0".as_ptr() as *const libc::c_void, 1);
            libc::close(old);
        }
    }
}
