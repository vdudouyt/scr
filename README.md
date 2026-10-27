# scr

A lightweight, detachable GNU Screen replacement with a Turbo Vision interface.

`scr` runs a small daemon per session, owns the PTYs, and renders the UI with
`ratatui` + `tui-term` + `vt100`. A foreground client attaches over a Unix
socket. Detaching closes only the client; shells, tabs and scrollback stay alive
until the daemon ends.

## Features

- Real detach/reattach, GNU Screen style (`Ctrl+A D`).
- Mouse-accessible tabs: click to switch, `×` to close, `+` to create.
- Hover feedback on tabs, close buttons, `+`, dialog rows and buttons.
- Full xterm key and mouse encoding: DECCKM arrows, modified keys, F1–F12,
  bracketed paste, SGR/X10 mouse protocols.
- Scrollback with a Turbo Vision scrollbar, wheel scrolling and scrollbar
  dragging.
- Drag-select text and copy it with OSC 52. `Shift`+drag selects even when the
  running program has grabbed the mouse.
- Session picker (`scr -r`) with keyboard and mouse.
- Turbo Vision palette: blue desktop, gray windows, cyan selection, red
  hotkeys, double-line dialogs with drop shadows.
- No async runtime, database, or network stack.

## Build

```sh
cargo build --release
install -m755 target/release/scr ~/.local/bin/scr
```

The project uses the same ratatui/tui-term/portable-pty/vt100 versions as the
OpenAdmin terminal stack (`ratatui 0.30.2`, `tui-term 0.3.4`, `vt100 0.16.2`,
`portable-pty 0.9.0`).

## Usage

```text
scr                       new session, then attach
scr <command> [args…]     new session running a command
scr -S <name>             new named session
scr -r [name]             resume a session; no name opens the picker
scr -ls                   list sessions
scr -d [name]             detach a session
scr -dr <name>            detach, then attach
scr --help                keys and options
```

With zero sessions, `scr -r` says so. With one detached session it attaches
directly, like GNU Screen. With several it opens the session picker.

A session name defaults to the client's pid. `scr -S work` names one.

## Keys

| Key | Action |
|---|---|
| `Ctrl+A C` / `Ctrl+A c` | new tab |
| `Ctrl+A D` / `Ctrl+A d` | detach |
| `Ctrl+A "` | select a tab from a list |
| `Ctrl+A N` / `Ctrl+A P` | next / previous tab |
| `Ctrl+A K` | close current tab |
| `Ctrl+A 1…9` | jump to tab (0 is the tenth) |
| `Alt+←` / `Alt+→` | previous / next tab |
| `Alt+1…9` | jump to tab |
| `Ctrl+A A` or `Ctrl+A Ctrl+A` | send a literal `Ctrl+A` |
| `Ctrl+A ?` | help dialog |

Everything else goes to the focused shell. `Alt+←/→` and `Alt+1…9` are reserved
by `scr`; the usual readline word-motion and numeric-argument bindings do not
reach the child.

## Mouse

- Click a tab to switch, `×` to close, `+` to create.
- Hover highlights the element under the pointer.
- The wheel scrolls the terminal back; typing or pasting returns to the prompt.
- Drag the scrollbar thumb, or click the track to jump.
- A program that enabled mouse reporting receives correctly encoded events.
  `Shift`+drag keeps selection for you.
- Dialogs are fully mouse-usable.

Selection copies on release using OSC 52. The terminal must allow OSC 52
clipboard writes for the copy to reach the system clipboard.

## Sessions and cleanup

State lives in:

```text
$XDG_RUNTIME_DIR/scr/     (fallback: /tmp/scr-$UID)
  <name>.sock
  <name>.lock
  <name>.log
```

The daemon holds an exclusive `flock` on `<name>.lock` for its whole life.
`scr -ls` and `scr -r` probe the socket; when the lock is free, the socket and
lock are stale and are removed. A hard kill cannot leave a session listed:

```sh
killall -9 scr
scr -ls            # No sessions.
```

The same is true after `kill -9` on one daemon: the kernel drops the lock, the
next listing prunes the entries, and the attached client reports that the
session terminated.

The daemon calls `setsid()` and ignores `SIGHUP`/`SIGPIPE`, so closing the
attaching terminal does not end the session. One client can be attached at a
time; `scr -d` detaches it.

## Layout

Two rows of chrome only:

```text
▐1:bash ×▌▐2:vim ×▌ +                                                  work
$ ls -la                                                                  ▲
src  Cargo.toml  README.md                                                █
$                                                                         ▼
 ^A C New   ^A D Detach   ^A " Tabs   ^A ? Help                work · 2 tabs
```

The terminal gets `rows - 2` and `cols - 1`; the last column is the scrollbar.
There is no window frame border.

## Architecture

- `src/client.rs` — attaches, forwards crossterm input, relays ANSI frames.
- `src/server/` — session daemon, tab state, event loop.
- `src/server/backend.rs` — a ratatui `Backend` that renders cell diffs into an
  ANSI byte stream over the socket.
- `src/term/` — PTY sessions, xterm key/mouse encoders, scrollback rules.
- `src/app.rs` — tabs, prefix state machine, mouse routing, selection, dialogs.
- `src/ui/` — Turbo Vision theme, tab strip, status line, pane, dialogs.
- `src/picker.rs` — `scr -r` session picker.
- `src/session.rs` — runtime directory, locks, discovery, daemon spawning.

Session output is parsed by `vt100` in the daemon. The daemon renders with
`ratatui` into the custom ANSI backend; the client never interprets screen
contents, so detach/reattach and multiple tabs stay consistent.

## Tests

```sh
cargo test
```

Unit coverage includes key/mouse encoders, scrollback, the tab/prefix state
machine, geometry, OSC 52 base64, session locking and the ANSI backend. The
end-to-end test runs a real client on a PTY against a real daemon: it opens tabs,
switches with Alt+1, detaches, reattaches, closes tabs, ends a session, and
verifies that a `SIGKILL`ed daemon disappears from the listing and removes its
socket and lock.
