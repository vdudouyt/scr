//! Client/server wire protocol.
//!
//! Messages are length-prefixed so the server can multiplex rendered ANSI
//! frames and control replies on one socket. Client messages are JSON; server
//! output frames carry raw terminal bytes.

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

pub const PROTO_VERSION: u16 = 1;
const MAX_FRAME: usize = 16 * 1024 * 1024;

pub const TAG_JSON: u8 = 0;
pub const TAG_ANSI: u8 = 1;
pub const TAG_DETACH: u8 = 2;
pub const TAG_EXIT: u8 = 3;
pub const TAG_INFO: u8 = 4;
pub const TAG_ERROR: u8 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabInfo {
    pub index: usize,
    pub title: String,
    pub active: bool,
    pub alive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub name: String,
    pub pid: u32,
    pub created: u64,
    pub attached: bool,
    pub tabs: Vec<TabInfo>,
    pub cwd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyMsg {
    pub code: KeyCodeMsg,
    pub mods: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KeyCodeMsg {
    Char(char),
    F(u8),
    Backspace,
    Enter,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Tab,
    BackTab,
    Delete,
    Insert,
    Esc,
    Null,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ButtonMsg {
    Left,
    Middle,
    Right,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MouseKindMsg {
    Down(ButtonMsg),
    Up(ButtonMsg),
    Drag(ButtonMsg),
    Moved,
    ScrollUp,
    ScrollDown,
    ScrollLeft,
    ScrollRight,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MouseMsg {
    pub kind: MouseKindMsg,
    pub column: u16,
    pub row: u16,
    pub mods: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMsg {
    Probe,
    Attach { version: u16, cols: u16, rows: u16 },
    Key(KeyMsg),
    Mouse(MouseMsg),
    Paste(String),
    Resize { cols: u16, rows: u16 },
    FocusLost,
    Detach,
}

impl KeyMsg {
    pub fn from_event(ev: KeyEvent) -> Option<Self> {
        if ev.kind != ratatui::crossterm::event::KeyEventKind::Press {
            return None;
        }
        let code = match ev.code {
            KeyCode::Char(c) => KeyCodeMsg::Char(c),
            KeyCode::F(n) => KeyCodeMsg::F(n),
            KeyCode::Backspace => KeyCodeMsg::Backspace,
            KeyCode::Enter => KeyCodeMsg::Enter,
            KeyCode::Left => KeyCodeMsg::Left,
            KeyCode::Right => KeyCodeMsg::Right,
            KeyCode::Up => KeyCodeMsg::Up,
            KeyCode::Down => KeyCodeMsg::Down,
            KeyCode::Home => KeyCodeMsg::Home,
            KeyCode::End => KeyCodeMsg::End,
            KeyCode::PageUp => KeyCodeMsg::PageUp,
            KeyCode::PageDown => KeyCodeMsg::PageDown,
            KeyCode::Tab => KeyCodeMsg::Tab,
            KeyCode::BackTab => KeyCodeMsg::BackTab,
            KeyCode::Delete => KeyCodeMsg::Delete,
            KeyCode::Insert => KeyCodeMsg::Insert,
            KeyCode::Esc => KeyCodeMsg::Esc,
            KeyCode::Null => KeyCodeMsg::Null,
            _ => return None,
        };
        Some(KeyMsg {
            code,
            mods: ev.modifiers.bits(),
        })
    }

    pub fn to_event(&self) -> KeyEvent {
        let code = match self.code {
            KeyCodeMsg::Char(c) => KeyCode::Char(c),
            KeyCodeMsg::F(n) => KeyCode::F(n),
            KeyCodeMsg::Backspace => KeyCode::Backspace,
            KeyCodeMsg::Enter => KeyCode::Enter,
            KeyCodeMsg::Left => KeyCode::Left,
            KeyCodeMsg::Right => KeyCode::Right,
            KeyCodeMsg::Up => KeyCode::Up,
            KeyCodeMsg::Down => KeyCode::Down,
            KeyCodeMsg::Home => KeyCode::Home,
            KeyCodeMsg::End => KeyCode::End,
            KeyCodeMsg::PageUp => KeyCode::PageUp,
            KeyCodeMsg::PageDown => KeyCode::PageDown,
            KeyCodeMsg::Tab => KeyCode::Tab,
            KeyCodeMsg::BackTab => KeyCode::BackTab,
            KeyCodeMsg::Delete => KeyCode::Delete,
            KeyCodeMsg::Insert => KeyCode::Insert,
            KeyCodeMsg::Esc => KeyCode::Esc,
            KeyCodeMsg::Null => KeyCode::Null,
        };
        KeyEvent::new(code, KeyModifiers::from_bits_truncate(self.mods))
    }
}

impl MouseMsg {
    pub fn from_event(ev: MouseEvent) -> Self {
        let kind = match ev.kind {
            MouseEventKind::Down(b) => MouseKindMsg::Down(button(b)),
            MouseEventKind::Up(b) => MouseKindMsg::Up(button(b)),
            MouseEventKind::Drag(b) => MouseKindMsg::Drag(button(b)),
            MouseEventKind::Moved => MouseKindMsg::Moved,
            MouseEventKind::ScrollUp => MouseKindMsg::ScrollUp,
            MouseEventKind::ScrollDown => MouseKindMsg::ScrollDown,
            MouseEventKind::ScrollLeft => MouseKindMsg::ScrollLeft,
            MouseEventKind::ScrollRight => MouseKindMsg::ScrollRight,
        };
        MouseMsg {
            kind,
            column: ev.column,
            row: ev.row,
            mods: ev.modifiers.bits(),
        }
    }

    pub fn to_event(&self) -> MouseEvent {
        let kind = match self.kind {
            MouseKindMsg::Down(b) => MouseEventKind::Down(button_out(b)),
            MouseKindMsg::Up(b) => MouseEventKind::Up(button_out(b)),
            MouseKindMsg::Drag(b) => MouseEventKind::Drag(button_out(b)),
            MouseKindMsg::Moved => MouseEventKind::Moved,
            MouseKindMsg::ScrollUp => MouseEventKind::ScrollUp,
            MouseKindMsg::ScrollDown => MouseEventKind::ScrollDown,
            MouseKindMsg::ScrollLeft => MouseEventKind::ScrollLeft,
            MouseKindMsg::ScrollRight => MouseEventKind::ScrollRight,
        };
        MouseEvent {
            kind,
            column: self.column,
            row: self.row,
            modifiers: KeyModifiers::from_bits_truncate(self.mods),
        }
    }
}

fn button(b: MouseButton) -> ButtonMsg {
    match b {
        MouseButton::Left => ButtonMsg::Left,
        MouseButton::Middle => ButtonMsg::Middle,
        MouseButton::Right => ButtonMsg::Right,
    }
}

fn button_out(b: ButtonMsg) -> MouseButton {
    match b {
        ButtonMsg::Left => MouseButton::Left,
        ButtonMsg::Middle => MouseButton::Middle,
        ButtonMsg::Right => MouseButton::Right,
    }
}

pub fn write_frame<W: Write>(w: &mut W, tag: u8, payload: &[u8]) -> io::Result<()> {
    if payload.len() > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "frame too large"));
    }
    let mut head = [0u8; 5];
    head[0] = tag;
    head[1..].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    w.write_all(&head)?;
    w.write_all(payload)?;
    w.flush()
}

pub fn read_frame<R: Read>(r: &mut R) -> io::Result<(u8, Vec<u8>)> {
    let mut head = [0u8; 5];
    r.read_exact(&mut head)?;
    let len = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload)?;
    Ok((head[0], payload))
}

pub fn write_json<W: Write, T: Serialize>(w: &mut W, tag: u8, value: &T) -> io::Result<()> {
    let body = serde_json::to_vec(value)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    write_frame(w, tag, &body)
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frames_round_trip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, TAG_ANSI, b"hello").unwrap();
        let mut c = Cursor::new(buf);
        assert_eq!(read_frame(&mut c).unwrap(), (TAG_ANSI, b"hello".to_vec()));
    }

    #[test]
    fn a_key_event_survives_the_wire() {
        let ev = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);
        let msg = KeyMsg::from_event(ev).unwrap();
        let back = msg.to_event();
        assert_eq!(back.code, KeyCode::Char('a'));
        assert!(back.modifiers.contains(KeyModifiers::CONTROL));
    }

    #[test]
    fn a_mouse_event_survives_the_wire() {
        let ev = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 12,
            row: 4,
            modifiers: KeyModifiers::ALT,
        };
        let msg = MouseMsg::from_event(ev);
        let back = msg.to_event();
        assert!(matches!(back.kind, MouseEventKind::ScrollUp));
        assert_eq!((back.column, back.row), (12, 4));
        assert!(back.modifiers.contains(KeyModifiers::ALT));
    }

    #[test]
    fn oversized_frames_are_refused() {
        let mut head = vec![TAG_ANSI];
        head.extend_from_slice(&(MAX_FRAME as u32 + 1).to_le_bytes());
        let mut c = Cursor::new(head);
        assert!(read_frame(&mut c).is_err());
    }
}
