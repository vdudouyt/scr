//! Attaching client: forwards input to the daemon and relays its ANSI frames.

use crate::proto::{self, ClientMsg, KeyMsg, MouseMsg, TAG_ANSI, TAG_DETACH, TAG_ERROR, TAG_EXIT, TAG_INFO};
use crate::session;
use crate::terminal;
use anyhow::{Context, Result, anyhow, bail};
use ratatui::crossterm::event::{self, Event};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

enum UiEvent {
    Input(Event),
    Detached,
    Exit(String),
    Error(String),
    Lost,
}

pub fn attach(name: &str) -> Result<()> {
    let (cols, rows) = ratatui::crossterm::terminal::size().context("read the terminal size")?;
    let mut stream = session::connect(name)?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .context("set handshake timeout")?;
    send(
        &mut stream,
        &ClientMsg::Attach {
            version: proto::PROTO_VERSION,
            cols,
            rows,
        },
    )?;
    match proto::read_frame(&mut stream) {
        Ok((TAG_INFO, _)) => {}
        Ok((TAG_ERROR, body)) => {
            bail!(String::from_utf8_lossy(&body).to_string());
        }
        Ok((tag, _)) => bail!("unexpected handshake frame {tag}"),
        Err(e) => bail!("session {name} did not answer: {e}"),
    }
    stream.set_read_timeout(None)?;

    terminal::install_panic_hook();
    terminal::enter().context("enter the terminal")?;
    let (tx, rx) = channel::<UiEvent>();
    let reader_stream = stream.try_clone().context("split the socket")?;
    spawn_reader(reader_stream, name.to_string(), tx.clone());
    spawn_input(tx.clone());

    let mut detached = false;
    let mut exit: Option<String> = None;
    let mut error: Option<String> = None;
    loop {
        match rx.recv() {
            Ok(UiEvent::Input(ev)) => {
                let msg = match ev {
                    Event::Key(k) => KeyMsg::from_event(k).map(ClientMsg::Key),
                    Event::Mouse(m) => Some(ClientMsg::Mouse(MouseMsg::from_event(m))),
                    Event::Paste(text) => Some(ClientMsg::Paste(text)),
                    Event::Resize(cols, rows) => Some(ClientMsg::Resize { cols, rows }),
                    Event::FocusGained => None,
                    Event::FocusLost => Some(ClientMsg::FocusLost),
                };
                if let Some(msg) = msg
                    && let Err(e) = send(&mut stream, &msg)
                {
                    error = Some(format!("lost the session: {e}"));
                    break;
                }
            }
            Ok(UiEvent::Detached) => {
                detached = true;
                break;
            }
            Ok(UiEvent::Exit(msg)) => {
                exit = Some(msg);
                break;
            }
            Ok(UiEvent::Error(msg)) => {
                error = Some(msg);
                break;
            }
            Ok(UiEvent::Lost) => {
                error = Some(format!("session {name} terminated"));
                break;
            }
            Err(_) => {
                error = Some(format!("session {name} terminated"));
                break;
            }
        }
    }

    terminal::leave();
    if detached {
        println!("[detached from {name}]");
        return Ok(());
    }
    if let Some(msg) = exit {
        println!("[{msg}]");
        return Ok(());
    }
    if let Some(msg) = error {
        return Err(anyhow!(msg));
    }
    Ok(())
}

fn send(stream: &mut UnixStream, msg: &ClientMsg) -> Result<()> {
    proto::write_json(stream, proto::TAG_JSON, msg).context("write to the session")
}

fn spawn_reader(mut stream: UnixStream, _name: String, tx: Sender<UiEvent>) {
    std::thread::Builder::new()
        .name("scr-client-reader".into())
        .spawn(move || {
            let mut out = std::io::stdout();
            loop {
                let (tag, body) = match proto::read_frame(&mut stream) {
                    Ok(f) => f,
                    Err(_) => {
                        let _ = tx.send(UiEvent::Lost);
                        return;
                    }
                };
                match tag {
                    TAG_ANSI => {
                        if out.write_all(&body).is_err() || out.flush().is_err() {
                            let _ = tx.send(UiEvent::Lost);
                            return;
                        }
                    }
                    TAG_DETACH => {
                        let _ = tx.send(UiEvent::Detached);
                        return;
                    }
                    TAG_EXIT => {
                        let msg = String::from_utf8_lossy(&body);
                        let _ = tx.send(UiEvent::Exit(msg.to_string()));
                        return;
                    }
                    TAG_ERROR => {
                        let msg = String::from_utf8_lossy(&body);
                        let _ = tx.send(UiEvent::Error(msg.to_string()));
                        return;
                    }
                    TAG_INFO => {}
                    _ => {}
                }
            }
        })
        .expect("spawn client reader");
}

fn spawn_input(tx: Sender<UiEvent>) {
    std::thread::Builder::new()
        .name("scr-input".into())
        .spawn(move || {
            loop {
                match event::read() {
                    Ok(ev) => {
                        if tx.send(UiEvent::Input(ev)).is_err() {
                            return;
                        }
                    }
                    Err(_) => return,
                }
            }
        })
        .expect("spawn input reader");
}
