//! Restores the terminal after the attaching client dies without unwinding.
//!
//! The client starts this binary with a pipe on stdin (see `src/terminal.rs`).
//! The first byte we write to stderr tells the client the original modes are
//! safely snapshotted; a `0` byte on stdin then means it restored the terminal
//! itself, while EOF — the client was killed, including with SIGKILL — means we
//! must restore it. We run in our own process group so terminal-generated
//! signals never reach us, and under the name `scr-guard` so `killall scr`
//! leaves us alive.

#[path = "../terminal/restore.rs"]
mod restore;

fn main() {
    restore::guard();
}
