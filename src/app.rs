//! Server-side application state: tabs, prefix handling, mouse routing and the
//! modal tab picker/help overlays.

use crate::proto::{SessionInfo, TabInfo};
use crate::term::keys;
use crate::term::session::{SessionId, Spawn, TermEvent, TerminalSession};
use crate::ui;
use anyhow::{Context, Result};
use ratatui::crossterm::clipboard::CopyToClipboard;
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::crossterm::queue;
use ratatui::layout::{Position, Rect};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const FLASH_TIME: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    TabPicker,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogButton {
    GoTo,
    Cancel,
    Ok,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoverTarget {
    None,
    Tab(usize),
    TabClose(usize),
    NewTab,
    Pane,
    DialogRow(usize),
    DialogButton(DialogButton),
}

#[derive(Debug, Default, Clone)]
pub struct Regions {
    pub tabs: Vec<(Rect, usize)>,
    pub tab_closes: Vec<(Rect, usize)>,
    pub new_tab: Option<Rect>,
    pub pane: Option<Rect>,
    pub dialog_rows: Vec<(Rect, usize)>,
    pub dialog_buttons: Vec<(Rect, DialogButton)>,
}

impl Regions {
    pub fn clear(&mut self) {
        self.tabs.clear();
        self.tab_closes.clear();
        self.new_tab = None;
        self.pane = None;
        self.dialog_rows.clear();
        self.dialog_buttons.clear();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub start: (u16, u16),
    pub end: (u16, u16),
    pub dragging: bool,
}

impl Selection {
    pub fn normalized(self) -> ((u16, u16), (u16, u16)) {
        if self.start <= self.end {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        }
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}

pub struct Tab {
    pub id: SessionId,
    pub title: String,
    pub session: TerminalSession,
}

pub struct TabPickerState {
    pub cursor: usize,
    pub scroll: usize,
}

pub struct App {
    pub name: String,
    pub created: u64,
    pub cwd: PathBuf,
    pub tabs: Vec<Tab>,
    pub active: usize,
    next_id: SessionId,
    initial: Vec<String>,
    shell: String,
    pub prefix: bool,
    pub mode: Mode,
    pub tab_picker: Option<TabPickerState>,
    pub hover: HoverTarget,
    pub regions: Regions,
    pub selection: Option<Selection>,
    pub flash: Option<(String, Instant)>,
    pub size: (u16, u16),
    pub should_quit: bool,
    pub detach_requested: bool,
    scrollback: usize,
    term: String,
    term_tx: Sender<TermEvent>,
    /// Immediate control bytes (currently OSC 52 clipboard writes) the server
    /// should hand to the attached client before its next rendered frame.
    pub outbox: Vec<u8>,
}

impl App {
    pub fn new(
        name: String,
        cwd: PathBuf,
        size: (u16, u16),
        scrollback: usize,
        term: String,
        term_tx: Sender<TermEvent>,
        initial: Vec<String>,
    ) -> Result<Self> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let created = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut app = App {
            name,
            created,
            cwd,
            tabs: Vec::new(),
            active: 0,
            next_id: 0,
            initial,
            shell,
            prefix: false,
            mode: Mode::Normal,
            tab_picker: None,
            hover: HoverTarget::None,
            regions: Regions::default(),
            selection: None,
            flash: None,
            size,
            should_quit: false,
            detach_requested: false,
            scrollback,
            term,
            term_tx,
            outbox: Vec::new(),
        };
        let first = if app.initial.is_empty() {
            None
        } else {
            Some(app.initial.clone())
        };
        app.add_tab(first)?;
        Ok(app)
    }

    // ---- layout and tabs -------------------------------------------------

    pub fn layout(&self) -> ui::Layout {
        ui::layout(Rect::new(0, 0, self.size.0, self.size.1))
    }

    pub fn pane_size(&self) -> (u16, u16) {
        let l = self.layout();
        (l.term.height.max(1), l.term.width.max(1))
    }

    pub fn set_size(&mut self, cols: u16, rows: u16) {
        if self.size == (cols, rows) {
            return;
        }
        self.size = (cols, rows);
        let (h, w) = self.pane_size();
        for tab in &mut self.tabs {
            let _ = tab.session.resize((h, w));
        }
        self.selection = None;
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    pub fn add_tab(&mut self, command: Option<Vec<String>>) -> Result<()> {
        let (rows, cols) = self.pane_size();
        let spawn = self.spawn_for(command.as_deref());
        let title = Path::new(&spawn.program)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| spawn.program.clone());
        self.next_id += 1;
        let id = self.next_id;
        let session = TerminalSession::spawn(
            id,
            title.clone(),
            &spawn,
            (rows, cols),
            self.scrollback,
            &self.term,
            self.term_tx.clone(),
        )
        .with_context(|| format!("start {title}"))?;
        self.tabs.push(Tab {
            id,
            title,
            session,
        });
        self.active = self.tabs.len() - 1;
        self.selection = None;
        self.hover = HoverTarget::None;
        Ok(())
    }

    fn spawn_for(&self, command: Option<&[String]>) -> Spawn {
        let (program, args) = match command {
            Some(cmd) if !cmd.is_empty() => (cmd[0].clone(), cmd[1..].to_vec()),
            _ => (self.shell.clone(), Vec::new()),
        };
        Spawn {
            program,
            args,
            env: vec![
                ("TERM".into(), self.term.clone()),
                ("COLORTERM".into(), "truecolor".into()),
            ],
            cwd: Some(self.cwd.clone()),
        }
    }

    pub fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        let mut tab = self.tabs.remove(index);
        tab.session.kill();
        if self.tabs.is_empty() {
            self.should_quit = true;
            self.active = 0;
        } else if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        } else if index < self.active {
            self.active -= 1;
        }
        self.selection = None;
        self.hover = HoverTarget::None;
    }

    pub fn close_active_tab(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        self.close_tab(self.active);
    }

    pub fn select_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active = index;
            self.selection = None;
            self.prefix = false;
        }
    }

    pub fn next_tab(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        self.select_tab((self.active + 1) % self.tabs.len());
    }

    pub fn prev_tab(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        self.select_tab((self.active + self.tabs.len() - 1) % self.tabs.len());
    }

    pub fn goto_tab_number(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        if n <= self.tabs.len() {
            self.select_tab(n - 1);
        }
    }

    pub fn reap_finished(&mut self) {
        while let Some(pos) = self
            .tabs
            .iter()
            .position(|t| t.session.has_exited())
        {
            self.close_tab(pos);
            if self.should_quit {
                return;
            }
        }
    }

    pub fn take_dirty(&mut self) -> bool {
        let mut dirty = false;
        for tab in &mut self.tabs {
            dirty |= tab.session.take_dirty();
        }
        dirty
    }

    /// Drop pointer/prefix state that belonged to the previous client.
    pub fn on_attach(&mut self) {
        self.prefix = false;
        self.hover = HoverTarget::None;
        self.selection = None;
    }

    pub fn info(&self, attached: bool) -> SessionInfo {
        SessionInfo {
            name: self.name.clone(),
            pid: std::process::id(),
            created: self.created,
            attached,
            tabs: self
                .tabs
                .iter()
                .enumerate()
                .map(|(i, t)| TabInfo {
                    index: i,
                    title: t.title.clone(),
                    active: i == self.active,
                    alive: !t.session.has_exited(),
                })
                .collect(),
            cwd: self.cwd.display().to_string(),
        }
    }

    // ---- status ----------------------------------------------------------

    pub fn flash(&mut self, text: impl Into<String>) {
        self.flash = Some((text.into(), Instant::now()));
    }

    pub fn flash_text(&self) -> Option<&str> {
        match &self.flash {
            Some((text, at)) if at.elapsed() < FLASH_TIME => Some(text),
            _ => None,
        }
    }

    // ---- input -----------------------------------------------------------

    pub fn on_key(&mut self, key: KeyEvent) {
        if self.mode == Mode::TabPicker {
            self.key_tab_picker(key);
            return;
        }
        if self.mode == Mode::Help {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter)
                || (key.code == KeyCode::Char('?') && key.modifiers.contains(KeyModifiers::CONTROL))
            {
                self.mode = Mode::Normal;
                self.prefix = false;
            }
            return;
        }

        // Alt+arrows and Alt+digit are the always-available tab keys.
        if !self.prefix && key.modifiers.contains(KeyModifiers::ALT) {
            match key.code {
                KeyCode::Left => {
                    self.prev_tab();
                    return;
                }
                KeyCode::Right => {
                    self.next_tab();
                    return;
                }
                KeyCode::Char(c @ '1'..='9') => {
                    self.goto_tab_number(c as usize - '0' as usize);
                    return;
                }
                _ => {}
            }
        }

        if self.prefix {
            self.prefix_key(key);
            return;
        }
        if key.code == KeyCode::Char('a') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.prefix = true;
            return;
        }
        self.forward_key(key);
    }

    fn prefix_key(&mut self, key: KeyEvent) {
        self.prefix = false;
        match key.code {
            KeyCode::Char('c' | 'C') => {
                if let Err(e) = self.add_tab(None) {
                    self.flash(format!("Could not open tab: {e}"));
                }
            }
            KeyCode::Char('d' | 'D') => {
                self.detach_requested = true;
            }
            KeyCode::Char('n' | 'N') => self.next_tab(),
            KeyCode::Char('p' | 'P') => self.prev_tab(),
            KeyCode::Char('k' | 'K') => self.close_active_tab(),
            KeyCode::Char('a' | 'A') => {
                self.write_focused(b"\x01");
            }
            KeyCode::Char('"') => self.open_tab_picker(),
            KeyCode::Char('?') => self.mode = Mode::Help,
            KeyCode::Char(c @ '0'..='9') => {
                let n = if c == '0' {
                    10
                } else {
                    c as usize - '0' as usize
                };
                self.goto_tab_number(n);
            }
            KeyCode::Esc => {}
            other => self.flash(format!("Unknown Ctrl+A command: {other:?}")),
        }
    }

    fn forward_key(&mut self, key: KeyEvent) {
        let Some(id) = self.active_tab().map(|t| t.id) else {
            return;
        };
        let app_cursor = self
            .tabs
            .iter()
            .find(|t| t.id == id)
            .and_then(|t| t.session.parser().lock().ok().map(|p| p.screen().application_cursor()))
            .unwrap_or(false);
        let bytes = keys::encode(key, app_cursor);
        self.write_focused(&bytes);
    }

    pub fn write_focused(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.selection = None;
        if let Some(tab) = self.tabs.get_mut(self.active) {
            if !tab.session.has_exited() {
                tab.session.scroll_to_live();
            }
            let _ = tab.session.write(bytes);
        }
    }

    pub fn on_paste(&mut self, text: &str) {
        if self.mode != Mode::Normal {
            return;
        }
        let bracketed = self
            .active_tab()
            .and_then(|t| t.session.parser().lock().ok().map(|p| p.screen().bracketed_paste()))
            .unwrap_or(false);
        let bytes = keys::encode_paste(text, bracketed);
        self.write_focused(&bytes);
    }

    fn open_tab_picker(&mut self) {
        self.tab_picker = Some(TabPickerState {
            cursor: self.active,
            scroll: 0,
        });
        self.mode = Mode::TabPicker;
    }

    fn key_tab_picker(&mut self, key: KeyEvent) {
        if self.tab_picker.is_none() {
            self.mode = Mode::Normal;
            return;
        }
        let n = self.tabs.len();
        let cursor = self.tab_picker.as_ref().map(|p| p.cursor).unwrap_or(0);
        let mut next = cursor;
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.tab_picker = None;
                return;
            }
            KeyCode::Enter => {
                self.commit_tab_picker();
                return;
            }
            KeyCode::Up => next = cursor.saturating_sub(1),
            KeyCode::Down => {
                if cursor + 1 < n {
                    next = cursor + 1;
                }
            }
            KeyCode::Home => next = 0,
            KeyCode::End => next = n.saturating_sub(1),
            KeyCode::PageUp => next = cursor.saturating_sub(8),
            KeyCode::PageDown => next = (cursor + 8).min(n.saturating_sub(1)),
            _ => {}
        }
        if let Some(p) = self.tab_picker.as_mut() {
            p.cursor = next;
        }
    }

    fn commit_tab_picker(&mut self) {
        let cursor = self.tab_picker.as_ref().map(|p| p.cursor).unwrap_or(0);
        self.mode = Mode::Normal;
        self.tab_picker = None;
        self.select_tab(cursor);
    }

    // ---- mouse -----------------------------------------------------------

    pub fn on_mouse(&mut self, ev: MouseEvent) {
        let at = Position::new(ev.column, ev.row);
        if ev.kind == MouseEventKind::Moved {
            let target = self.hit_test(at);
            if target != self.hover {
                self.hover = target;
            }
            // A program that asked for motion still gets it, but header and
            // dialog regions never do.
            if matches!(self.hover, HoverTarget::Pane) {
                self.forward_pane_mouse(&ev, at);
            }
            return;
        }

        self.prefix = false;
        if self.mode != Mode::Normal {
            self.mouse_dialog(&ev, at);
            return;
        }

        match ev.kind {
            MouseEventKind::ScrollUp => self.wheel(-3, at),
            MouseEventKind::ScrollDown => self.wheel(3, at),
            MouseEventKind::Down(MouseButton::Left) => self.click_left(at, ev.modifiers),
            MouseEventKind::Up(MouseButton::Left) => self.release_left(at, ev.modifiers),
            MouseEventKind::Drag(MouseButton::Left) => self.drag_left(at, ev.modifiers),
            _ => {
                if self.hover == HoverTarget::Pane {
                    self.forward_pane_mouse(&ev, at);
                }
            }
        }
    }

    fn hit_test(&self, at: Position) -> HoverTarget {
        if self.mode != Mode::Normal {
            for &(rect, i) in &self.regions.dialog_rows {
                if rect.contains(at) {
                    return HoverTarget::DialogRow(i);
                }
            }
            for &(rect, b) in &self.regions.dialog_buttons {
                if rect.contains(at) {
                    return HoverTarget::DialogButton(b);
                }
            }
            return HoverTarget::None;
        }
        for &(rect, i) in &self.regions.tab_closes {
            if rect.contains(at) {
                return HoverTarget::TabClose(i);
            }
        }
        for &(rect, i) in &self.regions.tabs {
            if rect.contains(at) {
                return HoverTarget::Tab(i);
            }
        }
        if self.regions.new_tab.is_some_and(|r| r.contains(at)) {
            return HoverTarget::NewTab;
        }
        if self.regions.pane.is_some_and(|r| r.contains(at)) {
            return HoverTarget::Pane;
        }
        HoverTarget::None
    }

    fn click_left(&mut self, at: Position, mods: KeyModifiers) {
        match self.hit_test(at) {
            HoverTarget::TabClose(i) => self.close_tab(i),
            HoverTarget::Tab(i) => self.select_tab(i),
            HoverTarget::NewTab => {
                if let Err(e) = self.add_tab(None) {
                    self.flash(format!("Could not open tab: {e}"));
                }
            }
            HoverTarget::Pane => {
                if !self.pane_wants_mouse() || mods.contains(KeyModifiers::SHIFT) {
                    self.start_selection(at);
                } else {
                    let ev = MouseEvent {
                        kind: MouseEventKind::Down(MouseButton::Left),
                        column: at.x,
                        row: at.y,
                        modifiers: mods,
                    };
                    self.forward_pane_mouse(&ev, at);
                }
            }
            _ => {}
        }
    }

    fn release_left(&mut self, at: Position, mods: KeyModifiers) {
        if let Some(sel) = self.selection.as_mut() {
            if sel.dragging {
                sel.dragging = false;
                if !sel.is_empty() {
                    self.copy_selection();
                } else {
                    self.selection = None;
                }
                return;
            }
        }
        if self.hover == HoverTarget::Pane
            && self.pane_wants_mouse()
            && !mods.contains(KeyModifiers::SHIFT)
        {
            let ev = MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: at.x,
                row: at.y,
                modifiers: mods,
            };
            self.forward_pane_mouse(&ev, at);
        }
    }

    fn drag_left(&mut self, at: Position, mods: KeyModifiers) {
        if let Some(sel) = self.selection.as_mut()
            && sel.dragging
        {
            let Some(pane) = self.regions.pane else {
                return;
            };
            let (x, y) = clamp_to_pane(at, pane);
            sel.end = (y, x);
            return;
        }
        if self.hover == HoverTarget::Pane && self.pane_wants_mouse() {
            let ev = MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: at.x,
                row: at.y,
                modifiers: mods,
            };
            self.forward_pane_mouse(&ev, at);
        }
    }

    fn start_selection(&mut self, at: Position) {
        let Some(pane) = self.regions.pane else {
            return;
        };
        let (x, y) = clamp_to_pane(at, pane);
        self.selection = Some(Selection {
            start: (y, x),
            end: (y, x),
            dragging: true,
        });
    }

    fn copy_selection(&mut self) {
        let Some(sel) = self.selection else {
            return;
        };
        let ((sr, sc), (er, ec)) = sel.normalized();
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let text = match tab.session.parser().lock() {
            Ok(p) => p.screen().contents_between(sr, sc, er, ec),
            Err(_) => return,
        };
        let text = text
            .lines()
            .map(|l| l.trim_end())
            .collect::<Vec<_>>()
            .join("\n");
        let text = text.trim_end_matches('\n').to_string();
        if text.is_empty() {
            self.flash("Nothing selected");
            return;
        }
        let count = text.chars().count();
        let mut bytes = Vec::new();
        if queue!(bytes, CopyToClipboard::to_clipboard_from(text)).is_err() {
            self.flash("Could not copy selection");
            return;
        }
        self.outbox.extend_from_slice(&bytes);
        self.flash(format!("Copied {count} chars"));
    }

    fn wheel(&mut self, lines: isize, at: Position) {
        if self.mode != Mode::Normal {
            return;
        }
        if !self.regions.pane.is_some_and(|r| r.contains(at)) {
            return;
        }
        if self.pane_wants_mouse() {
            let ev = MouseEvent {
                kind: if lines < 0 {
                    MouseEventKind::ScrollUp
                } else {
                    MouseEventKind::ScrollDown
                },
                column: at.x,
                row: at.y,
                modifiers: KeyModifiers::empty(),
            };
            self.forward_pane_mouse(&ev, at);
            return;
        }
        self.selection = None;
        if let Some(tab) = self.tabs.get_mut(self.active) {
            // Up the screen is back in time; `scroll_history` takes positive
            // values as backward.
            tab.session.scroll_history(-lines);
        }
    }

    fn pane_wants_mouse(&self) -> bool {
        let Some(tab) = self.tabs.get(self.active) else {
            return false;
        };
        match tab.session.parser().lock() {
            Ok(p) => p.screen().mouse_protocol_mode() != vt100::MouseProtocolMode::None,
            Err(_) => false,
        }
    }

    fn forward_pane_mouse(&mut self, ev: &MouseEvent, at: Position) -> bool {
        let Some(pane) = self.regions.pane else {
            return false;
        };
        if !pane.contains(at) {
            return false;
        }
        let (mode, encoding) = {
            let Some(tab) = self.tabs.get(self.active) else {
                return false;
            };
            match tab.session.parser().lock() {
                Ok(p) => (
                    p.screen().mouse_protocol_mode(),
                    p.screen().mouse_protocol_encoding(),
                ),
                Err(_) => return false,
            }
        };
        let col = at.x.saturating_sub(pane.x);
        let row = at.y.saturating_sub(pane.y);
        let Some(bytes) = keys::encode_mouse(ev, col, row, mode, encoding) else {
            return false;
        };
        if let Some(tab) = self.tabs.get_mut(self.active) {
            let _ = tab.session.write(&bytes);
        }
        true
    }

    fn mouse_dialog(&mut self, ev: &MouseEvent, at: Position) {
        if ev.kind != MouseEventKind::Down(MouseButton::Left) {
            return;
        }
        match self.hit_test(at) {
            HoverTarget::DialogRow(i) => {
                if self.mode == Mode::TabPicker {
                    if let Some(p) = self.tab_picker.as_mut() {
                        p.cursor = i;
                    }
                    self.commit_tab_picker();
                }
            }
            HoverTarget::DialogButton(DialogButton::GoTo) if self.mode == Mode::TabPicker => {
                self.commit_tab_picker();
            }
            HoverTarget::DialogButton(DialogButton::Cancel) if self.mode == Mode::TabPicker => {
                self.mode = Mode::Normal;
                self.tab_picker = None;
            }
            HoverTarget::DialogButton(DialogButton::Ok) if self.mode == Mode::Help => {
                self.mode = Mode::Normal;
            }
            _ => {}
        }
    }

    // ---- server-facing ---------------------------------------------------

    pub fn take_outbox(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.outbox)
    }
}

fn clamp_to_pane(at: Position, pane: Rect) -> (u16, u16) {
    let x = at
        .x
        .saturating_sub(pane.x)
        .min(pane.width.saturating_sub(1));
    let y = at
        .y
        .saturating_sub(pane.y)
        .min(pane.height.saturating_sub(1));
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_is_normalized() {
        let s = Selection {
            start: (4, 5),
            end: (1, 2),
            dragging: false,
        };
        assert_eq!(s.normalized(), ((1, 2), (4, 5)));
        assert!(!s.is_empty());
    }
}

#[cfg(test)]
mod app_tests {
    use super::*;
    use std::sync::mpsc::channel;

    fn app() -> App {
        let (tx, _rx) = channel();
        App::new(
            "test".into(),
            PathBuf::from("/"),
            (80, 24),
            1000,
            "xterm-256color".into(),
            tx,
            vec!["/bin/sh".into(), "-c".into(), "sleep 30".into()],
        )
        .unwrap()
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn the_first_tab_gets_the_pane_size_not_the_full_frame() {
        let a = app();
        assert_eq!(a.tabs.len(), 1);
        assert_eq!(a.tabs[0].session.size(), (23, 80));
    }

    #[test]
    fn tabs_open_cycle_and_close() {
        let mut a = app();
        a.add_tab(None).unwrap();
        assert_eq!(a.tabs.len(), 2);
        assert_eq!(a.active, 1);
        a.next_tab();
        assert_eq!(a.active, 0);
        a.prev_tab();
        assert_eq!(a.active, 1);
        a.goto_tab_number(1);
        assert_eq!(a.active, 0);
        a.close_active_tab();
        assert_eq!(a.tabs.len(), 1);
        assert!(!a.should_quit);
        a.close_active_tab();
        assert!(a.tabs.is_empty());
        assert!(a.should_quit);
    }

    #[test]
    fn the_screen_prefix_commands_do_what_they_say() {
        let mut a = app();
        a.on_key(ctrl('a'));
        assert!(a.prefix);
        a.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::empty()));
        assert!(!a.prefix);
        assert_eq!(a.tabs.len(), 2);
        a.on_key(ctrl('a'));
        a.on_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::empty()));
        assert!(a.detach_requested);
        a.detach_requested = false;
        a.on_key(ctrl('a'));
        a.on_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::empty()));
        assert_eq!(a.active, 0);
        a.on_key(ctrl('a'));
        a.on_key(KeyEvent::new(KeyCode::Char('"'), KeyModifiers::SHIFT));
        assert_eq!(a.mode, Mode::TabPicker);
    }

    #[test]
    fn alt_digits_select_tabs() {
        let mut a = app();
        a.add_tab(None).unwrap();
        a.on_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::ALT));
        assert_eq!(a.active, 0);
        a.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT));
        assert_eq!(a.active, 1);
    }

    #[test]
    fn copy_selection_sends_an_osc52_sequence() {
        let mut a = app();
        {
            let s = &a.tabs[0].session;
            let mut p = s.parser().lock().unwrap();
            p.process(b"hello world\r\n");
        }
        a.selection = Some(Selection {
            start: (0, 0),
            end: (0, 5),
            dragging: false,
        });
        a.copy_selection();
        let out = String::from_utf8_lossy(&a.outbox);
        assert!(out.starts_with("\x1b]52;c;"));
        assert!(out.contains("aGVsbG8=") || out.contains("aGVsbG8"));
    }

    #[test]
    fn layout_spends_only_the_tab_strip_on_chrome() {
        let a = app();
        assert_eq!(a.layout().header, Rect::new(0, 0, 80, 1));
        assert_eq!(a.layout().term, Rect::new(0, 1, 80, 23));
    }

    #[test]
    fn hover_changes_the_rendered_tab_styles() {
        use crate::ui::theme;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut a = app();
        a.add_tab(None).unwrap();
        let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();

        a.hover = HoverTarget::Tab(1);
        term.draw(|f| crate::ui::draw(f, &mut a)).unwrap();
        let active = a.regions.tabs.iter().find(|(_, i)| *i == 1).unwrap().0;
        assert_eq!(term.backend().buffer()[(active.x + 1, 0)].bg, theme::CYAN_BRIGHT);

        a.hover = HoverTarget::Tab(0);
        term.draw(|f| crate::ui::draw(f, &mut a)).unwrap();
        let idle = a.regions.tabs.iter().find(|(_, i)| *i == 0).unwrap().0;
        assert_eq!(term.backend().buffer()[(idle.x + 1, 0)].bg, theme::WHITE);

        a.hover = HoverTarget::TabClose(0);
        term.draw(|f| crate::ui::draw(f, &mut a)).unwrap();
        let close = a
            .regions
            .tab_closes
            .iter()
            .find(|(_, i)| *i == 0)
            .unwrap()
            .0;
        assert_eq!(term.backend().buffer()[(close.x, 0)].bg, theme::RED);

        a.hover = HoverTarget::NewTab;
        term.draw(|f| crate::ui::draw(f, &mut a)).unwrap();
        let plus = a.regions.new_tab.unwrap();
        assert_eq!(term.backend().buffer()[(plus.x, 0)].bg, theme::WHITE);
    }
}
