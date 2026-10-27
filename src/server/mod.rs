//! The session daemon: owns the PTYs and draws the UI into ANSI frames.

pub mod backend;

use self::backend::AnsiBackend;
use crate::app::App;
use crate::proto::{self, ClientMsg, TAG_ANSI, TAG_DETACH, TAG_ERROR, TAG_EXIT, TAG_INFO};
use crate::session;
use crate::term::session::TermEvent;
use crate::ui;
use anyhow::{Context, Result};
use ratatui::Terminal;
use ratatui::layout::Rect;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

enum Event {
    Conn(UnixStream),
    Client(u64, ClientMsg),
    ClientGone(u64),
    Term(TermEvent),
}

struct Client {
    stream: UnixStream,
    generation: u64,
    terminal: Terminal<AnsiBackend>,
}

pub fn run(name: String, initial: Vec<String>) -> Result<()> {
    session::validate_name(&name)?;
    unsafe {
        libc::setsid();
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }

    let lock = session::acquire_lock(&name)?;
    let dir = session::runtime_dir()?;
    let sock_path = dir.join(format!("{name}.sock"));
    let _ = std::fs::remove_file(&sock_path);
    let listener = UnixListener::bind(&sock_path)
        .with_context(|| format!("bind {}", sock_path.display()))?;
    let _ = std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600));
    let _guard = SocketGuard {
        path: sock_path,
        _lock: lock,
    };

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let (event_tx, event_rx) = channel::<Event>();
    let (term_tx, term_rx) = channel::<TermEvent>();
    spawn_term_bridge(term_rx, event_tx.clone());
    spawn_listener(listener, event_tx.clone());

    let mut app = App::new(
        name.clone(),
        cwd,
        (80, 24),
        10_000,
        "xterm-256color".into(),
        term_tx,
        initial,
    )
    .context("start the initial tab")?;

    event_loop(&mut app, event_rx, event_tx)
}

fn spawn_listener(listener: UnixListener, tx: Sender<Event>) {
    std::thread::Builder::new()
        .name("scr-listener".into())
        .spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(s) => {
                        if tx.send(Event::Conn(s)).is_err() {
                            return;
                        }
                    }
                    Err(_) => return,
                }
            }
        })
        .expect("spawn listener thread");
}

fn spawn_term_bridge(rx: Receiver<TermEvent>, tx: Sender<Event>) {
    std::thread::Builder::new()
        .name("scr-term-bridge".into())
        .spawn(move || {
            while let Ok(ev) = rx.recv() {
                if tx.send(Event::Term(ev)).is_err() {
                    return;
                }
            }
        })
        .expect("spawn term bridge");
}

fn spawn_client_reader(stream: UnixStream, generation: u64, tx: Sender<Event>) {
    std::thread::Builder::new()
        .name("scr-client-reader".into())
        .spawn(move || {
            let mut stream = stream;
            loop {
                let msg = match proto::read_frame(&mut stream) {
                    Ok((proto::TAG_JSON, body)) => serde_json::from_slice::<ClientMsg>(&body).ok(),
                    Ok(_) => None,
                    Err(_) => {
                        let _ = tx.send(Event::ClientGone(generation));
                        return;
                    }
                };
                match msg {
                    Some(msg) => {
                        if tx.send(Event::Client(generation, msg)).is_err() {
                            return;
                        }
                    }
                    None => {
                        let _ = tx.send(Event::ClientGone(generation));
                        return;
                    }
                }
            }
        })
        .expect("spawn client reader");
}

fn event_loop(app: &mut App, rx: Receiver<Event>, event_tx: Sender<Event>) -> Result<()> {
    let mut client: Option<Client> = None;
    let mut next_generation = 0u64;
    let mut dirty = true;

    loop {
        if app.detach_requested {
            app.detach_requested = false;
            detach_client(&mut client);
        }
        if app.should_quit {
            if let Some(c) = client.as_mut() {
                let _ = send_text(&mut c.stream, TAG_EXIT, &format!("session {} ended", app.name));
            }
            detach_client(&mut client);
            return Ok(());
        }
        if app.flash.is_some() && app.flash_text().is_none() {
            app.flash = None;
            dirty = true;
        }

        if dirty {
            if let Some(c) = client.as_mut() {
                if let Err(e) = draw(c, app) {
                    session::log_error(&app.name, &e);
                    detach_client(&mut client);
                }
                dirty = false;
            }
        }

        match rx.recv_timeout(Duration::from_millis(80)) {
            Ok(Event::Conn(stream)) => {
                handle_conn(
                    stream,
                    app,
                    &mut client,
                    &mut next_generation,
                    &mut dirty,
                    event_tx.clone(),
                );
            }
            Ok(Event::Client(generation, msg)) => {
                let Some(c) = client.as_mut() else {
                    continue;
                };
                if c.generation != generation {
                    continue;
                }
                let mut redraw = true;
                match msg {
                    ClientMsg::Key(k) => app.on_key(k.to_event()),
                    ClientMsg::Mouse(m) => {
                        let moved = matches!(m.kind, proto::MouseKindMsg::Moved);
                        let before = app.hover;
                        app.on_mouse(m.to_event());
                        // A pure motion event that did not change the hover
                        // target needs no repaint.
                        redraw = !moved || app.hover != before;
                    }
                    ClientMsg::Paste(text) => app.on_paste(&text),
                    ClientMsg::Resize { cols, rows } => {
                        app.set_size(cols, rows);
                        if let Some(c) = client.as_mut() {
                            let _ = c.terminal.backend_mut().resize(cols, rows);
                            let _ = c.terminal.resize(Rect::new(0, 0, cols.max(1), rows.max(1)));
                        }
                    }
                    ClientMsg::FocusLost => {
                        app.hover = crate::app::HoverTarget::None;
                        app.prefix = false;
                    }
                    ClientMsg::Detach => app.detach_requested = true,
                    ClientMsg::Probe | ClientMsg::Attach { .. } => {}
                }
                let out = app.take_outbox();
                if !out.is_empty() {
                    if let Some(c) = client.as_mut() {
                        let _ = proto::write_frame(&mut c.stream, TAG_ANSI, &out);
                    }
                }
                if redraw {
                    dirty = true;
                }
            }
            Ok(Event::ClientGone(generation)) => {
                if client.as_ref().is_some_and(|c| c.generation == generation) {
                    detach_client(&mut client);
                    dirty = true;
                }
            }
            Ok(Event::Term(ev)) => {
                if let TermEvent::Exit(_) = ev {
                    app.reap_finished();
                }
                if client.is_some() {
                    dirty = true;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if app.take_dirty() && client.is_some() {
            dirty = true;
        }
        app.reap_finished();
    }
    detach_client(&mut client);
    Ok(())
}

fn handle_conn(
    mut stream: UnixStream,
    app: &mut App,
    client: &mut Option<Client>,
    next_generation: &mut u64,
    dirty: &mut bool,
    event_tx: Sender<Event>,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let msg = match proto::read_frame(&mut stream) {
        Ok((proto::TAG_JSON, body)) => serde_json::from_slice::<ClientMsg>(&body).ok(),
        _ => None,
    };
    let _ = stream.set_read_timeout(None);
    let Some(msg) = msg else {
        let _ = send_text(&mut stream, TAG_ERROR, "bad request");
        return;
    };
    match msg {
        ClientMsg::Probe => {
            let _ = proto::write_json(&mut stream, TAG_INFO, &app.info(client.is_some()));
        }
        ClientMsg::Detach => {
            if client.is_some() {
                detach_client(client);
            }
            let _ = proto::write_json(&mut stream, TAG_INFO, &app.info(false));
            *dirty = true;
        }
        ClientMsg::Attach { version, cols, rows } => {
            if version != proto::PROTO_VERSION {
                let _ = send_text(
                    &mut stream,
                    TAG_ERROR,
                    "this scr client and session daemon are different versions; detach and restart the session",
                );
                return;
            }
            if client.is_some() {
                let _ = send_text(
                    &mut stream,
                    TAG_ERROR,
                    &format!(
                        "session {} is attached. Detach it with: scr -d {}",
                        app.name, app.name
                    ),
                );
                return;
            }
            let cols = cols.max(1);
            let rows = rows.max(1);
            app.set_size(cols, rows);
            app.on_attach();
            *next_generation += 1;
            let generation = *next_generation;
            if proto::write_json(&mut stream, TAG_INFO, &app.info(true)).is_err() {
                return;
            }
            let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
            let terminal = match Terminal::new(AnsiBackend::new(cols, rows)) {
                Ok(t) => t,
                Err(e) => {
                    let _ = send_text(&mut stream, TAG_ERROR, &format!("terminal init: {e}"));
                    return;
                }
            };
            let Ok(reader) = stream.try_clone() else {
                let _ = send_text(&mut stream, TAG_ERROR, "could not split the socket");
                return;
            };
            spawn_client_reader(reader, generation, event_tx);
            let _ = proto::write_frame(&mut stream, TAG_ANSI, b"\x1b[2J\x1b[H");
            *client = Some(Client {
                stream,
                generation,
                terminal,
            });
            *dirty = true;
        }
        ClientMsg::Key(_)
        | ClientMsg::Mouse(_)
        | ClientMsg::Paste(_)
        | ClientMsg::Resize { .. }
        | ClientMsg::FocusLost => {
            let _ = send_text(&mut stream, TAG_ERROR, "attach first");
        }
    }
}

// The listener and reader need the same event sender.
fn draw(c: &mut Client, app: &mut App) -> Result<()> {
    c.terminal
        .draw(|f| ui::draw(f, app))
        .context("draw a frame")?;
    let out = c.terminal.backend_mut().take();
    if !out.is_empty() {
        proto::write_frame(&mut c.stream, TAG_ANSI, &out).context("write a frame")?;
    }
    Ok(())
}

fn detach_client(client: &mut Option<Client>) {
    if let Some(mut c) = client.take() {
        let _ = send_text(&mut c.stream, TAG_DETACH, "");
        let _ = c.stream.shutdown(std::net::Shutdown::Both);
    }
}

fn send_text(stream: &mut UnixStream, tag: u8, text: &str) -> std::io::Result<()> {
    proto::write_frame(stream, tag, text.as_bytes())?;
    stream.flush()
}

struct SocketGuard {
    path: PathBuf,
    _lock: session::SessionLock,
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
