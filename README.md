# scr

A lightweight, detachable GNU Screen replacement with a modern grayscale TUI.

`scr` runs a small daemon per session, owns the PTYs, and renders the UI with
`ratatui` + `tui-term` + `vt100`. A foreground client attaches over a Unix
socket. Detaching closes only the client; shells, tabs and scrollback stay alive
until the daemon ends.

## Features

- Real detach/reattach, GNU Screen style (`Ctrl+A D`).
- Mouse-accessible tabs: click to switch, `×` to close, `+` to create.
- Tabs follow the window title the program sets (OSC 0/2); `e` in the tab
  list renames one, and a manual name outranks later title changes.
- Hover feedback on tabs, close buttons, `+`, dialog rows and buttons.
- Full xterm key and mouse encoding: DECCKM arrows, modified keys, F1–F12,
  bracketed paste, SGR/X10 mouse protocols.
- Scrollback with the mouse wheel; no scrollbar chrome.
- Session picker (`scr -r`) with keyboard and mouse.
- Modern grayscale UI: gray text on dark-gray chrome, inverted tab headers and
  borderless dialogs. The terminal pane keeps the terminal's own colours and
  background.
- No async runtime, database, or network stack.

## Build

```sh
cargo build --release
install -m755 target/release/scr ~/.local/bin/
```

The project uses the same ratatui/tui-term/portable-pty/vt100 versions as the
OpenAdmin terminal stack (`ratatui 0.30.2`, `tui-term 0.3.4`, `vt100 0.16.2`,
`portable-pty 0.9.0`). Terminal control and rendering use ratatui's crossterm
re-export, exactly as OpenAdmin does.

## Usage

```text
scr                       new session, then attach
scr <command> [args…]     new session running a command
scr -S <name>             new named session, or attach if it already exists
scr -r [name]             resume a session; no name opens the picker
scr -ls                   list sessions
scr --help                keys and options
```

With zero sessions, `scr -r` says so. With one session it attaches directly,
like GNU Screen. With several it opens the session picker.

A session name defaults to the client's pid. `scr -S work` names one. Naming an
existing session reuses it instead of starting a second daemon.

## Keys

| Key | Action |
|---|---|
| `Ctrl+A C` / `Ctrl+A c` | new tab |
| `Ctrl+A D` / `Ctrl+A d` | detach |
| `Ctrl+A "` | select a tab from a list |
| `e` | in the tab list, rename the selected tab (empty clears) |
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
- A program that enabled mouse reporting receives correctly encoded events.
- Dialogs are fully mouse-usable.

## Sessions and cleanup

State lives in:

```text
$XDG_RUNTIME_DIR/scr/     (fallback: ~/.scr, then /tmp/scr)
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

The daemon is started in its own process group and holds no terminal
descriptors, so closing the attaching terminal does not end the session. One
client can be attached at a time: attaching to a session that is already
attached detaches the other client and hands the session over. `Ctrl+A D`
detaches from inside the client.

The client runs on the alternate screen and restores termios, the screen, mouse
modes and cursor on detach, exit and panics. A `SIGKILL` cannot run cleanup, so
`killall -9 scr` leaves the terminal in the alternate screen; run `reset` to
recover.

## Layout

One row of chrome only:

```text
 1:bash ×  2:vim ×  +
$ ls -la
src  Cargo.toml  README.md
$
```

The terminal gets every row below the tab strip and the full width. There is no
status bar, no scrollbar column and no window frame border. Inactive tabs sit on
the bar's own background; the active tab is a subtly raised dark panel, and
dialogs are borderless panels over the terminal. The terminal pane itself
leaves default colours to the terminal, so its background and transparency are
untouched.

## Architecture

- `src/client.rs` — attaches, forwards crossterm input, relays ANSI frames.
- `src/server/` — session daemon, tab state, event loop.
- `src/server/backend.rs` — `CrosstermBackend` (the same encoder OpenAdmin
  uses) wrapped around a socket buffer, with the size and cursor position
  supplied by the attached client.
- `src/term/` — PTY sessions, xterm key/mouse encoders, scrollback rules.
- `src/app.rs` — tabs, prefix state machine, mouse routing, dialogs.
- `src/ui/` — grayscale theme, tab strip, pane, borderless dialogs.
- `src/picker.rs` — `scr -r` session picker.
- `src/terminal.rs` — raw mode and alternate screen setup.
- `src/terminal/restore.rs` — screen restoration.
- `src/session.rs` — runtime directory, locks, discovery, daemon spawning.

Session output is parsed by `vt100` in the daemon. The daemon renders with
`ratatui` through `CrosstermBackend` into the socket; the client never
interprets screen contents, so detach/reattach and multiple tabs stay
consistent.

Raw mode and the alternate screen use `ratatui::try_init()` / `try_restore()`,
the same pair OpenAdmin uses; mouse/paste/focus reporting comes from crossterm,
and the daemon's ANSI output is `CrosstermBackend`'s. The remaining process-level
work uses safe Rust APIs: the daemon gets its own process group through
`CommandExt::process_group`. The crate sets `unsafe_code = "forbid"`, so no
unsafe block can come back unnoticed.

## Tests

```sh
cargo test
```

Unit coverage includes key/mouse encoders, scrollback, the tab/prefix state
machine, geometry, session locking and the backend wrapper. The end-to-end test
runs a real client on a PTY against a real daemon: it opens tabs, switches with
Alt+1, detaches, reattaches, closes tabs, ends a session, and verifies that a
`SIGKILL`ed daemon disappears from the listing and removes its socket and lock.
A second end-to-end test verifies that SIGKILL leaves the alternate screen and
mouse modes restored.
