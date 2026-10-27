//! `scr -r` session picker, styled as a borderless dialog.

use crate::proto::SessionInfo;
use crate::terminal;
use crate::ui;
use crate::ui::theme;
use anyhow::Result;
use crate::server::backend::AnsiBackend;
use ratatui::Terminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hover {
    None,
    Row(usize),
}

struct State {
    cursor: usize,
    scroll: usize,
    hover: Hover,
    message: Option<String>,
    rows: Vec<(Rect, usize)>,
    last_click: Option<(Instant, usize)>,
}

pub fn pick(sessions: Vec<SessionInfo>) -> Result<Option<String>> {
    terminal::install_panic_hook();
    terminal::enter()?;
    let (cols, rows) = ratatui::crossterm::terminal::size().unwrap_or((80, 24));
    let mut term: Terminal<AnsiBackend> = Terminal::new(AnsiBackend::stdout(cols, rows))?;
    let mut state = State {
        cursor: 0,
        scroll: 0,
        hover: Hover::None,
        message: None,
        rows: Vec::new(),
        last_click: None,
    };
    let mut chosen = None;
    loop {
        term.draw(|f| draw(f, &sessions, &mut state))?;
        let ev = event::read()?;
        match ev {
            Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Esc | KeyCode::Char('q') => break,
                KeyCode::Enter => {
                    if let Some(name) = choose(&sessions, &mut state) {
                        chosen = Some(name);
                        break;
                    }
                }
                KeyCode::Up => state.cursor = state.cursor.saturating_sub(1),
                KeyCode::Down => {
                    if state.cursor + 1 < sessions.len() {
                        state.cursor += 1;
                    }
                }
                KeyCode::Home => state.cursor = 0,
                KeyCode::End => state.cursor = sessions.len().saturating_sub(1),
                KeyCode::PageUp => state.cursor = state.cursor.saturating_sub(8),
                KeyCode::PageDown => {
                    state.cursor = (state.cursor + 8).min(sessions.len().saturating_sub(1));
                }
                _ => {}
            },
            Event::Mouse(m) => match m.kind {
                MouseEventKind::Moved => {
                    let at = Position::new(m.column, m.row);
                    let h = hit(&state, at);
                    if h != state.hover {
                        state.hover = h;
                    }
                }
                MouseEventKind::ScrollUp => state.cursor = state.cursor.saturating_sub(1),
                MouseEventKind::ScrollDown => {
                    if state.cursor + 1 < sessions.len() {
                        state.cursor += 1;
                    }
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    let at = Position::new(m.column, m.row);
                    match hit(&state, at) {
                        Hover::Row(i) => {
                            let now = Instant::now();
                            let double = state
                                .last_click
                                .is_some_and(|(t, j)| j == i && now.duration_since(t) < Duration::from_millis(420));
                            state.cursor = i;
                            state.last_click = Some((now, i));
                            if double
                                && let Some(name) = choose(&sessions, &mut state)
                            {
                                chosen = Some(name);
                                break;
                            }
                        }
                        Hover::None => {}
                    }
                }
                _ => {}
            },
            Event::Resize(cols, rows) => {
                term.backend_mut().resize(cols, rows);
                let _ = term.resize(Rect::new(0, 0, cols.max(1), rows.max(1)));
            }
            _ => {}
        }
    }
    terminal::leave();
    Ok(chosen)
}

fn choose(sessions: &[SessionInfo], state: &mut State) -> Option<String> {
    let Some(s) = sessions.get(state.cursor) else {
        return None;
    };
    if s.attached {
        state.message = Some(format!(
            "{} is attached — detach it with: scr -d {}",
            s.name, s.name
        ));
        return None;
    }
    Some(s.name.clone())
}

fn hit(state: &State, at: Position) -> Hover {
    for &(rect, i) in &state.rows {
        if rect.contains(at) {
            return Hover::Row(i);
        }
    }
    Hover::None
}

fn draw(f: &mut ratatui::Frame, sessions: &[SessionInfo], state: &mut State) {
    let screen = f.area();
    f.render_widget(Block::new().style(theme::base()), screen);
    state.rows.clear();
    if sessions.is_empty() {
        return;
    }
    let w = 74.min(screen.width.saturating_sub(2)).max(30.min(screen.width));
    let h = (sessions.len() as u16 + 6)
        .min(screen.height.saturating_sub(2))
        .max(8.min(screen.height));
    if w == 0 || h == 0 {
        return;
    }
    let rect = Rect::new(
        screen.x + (screen.width.saturating_sub(w)) / 2,
        screen.y + (screen.height.saturating_sub(h)) / 2,
        w,
        h,
    );
    ui::dialogs::dialog(f, rect, "scr sessions");
    let inside = ui::dialogs::inner(rect);
    let rows = inside.height.saturating_sub(3) as usize;
    if rows == 0 {
        return;
    }
    if state.cursor >= sessions.len() {
        state.cursor = sessions.len() - 1;
    }
    if state.cursor < state.scroll {
        state.scroll = state.cursor;
    }
    if state.cursor >= state.scroll + rows {
        state.scroll = state.cursor + 1 - rows;
    }
    for (row, i) in (state.scroll..sessions.len().min(state.scroll + rows)).enumerate() {
        let s = &sessions[i];
        let y = inside.y + row as u16;
        let line_rect = Rect::new(inside.x, y, inside.width, 1);
        state.rows.push((line_rect, i));
        let selected = i == state.cursor;
        let hovered = state.hover == Hover::Row(i);
        let base = if selected {
            theme::list_selected()
        } else if hovered {
            theme::list_hover()
        } else {
            theme::dialog_bg()
        };
        let name = clip(&s.name, 18);
        let state_text = if s.attached { "attached" } else { "detached" };
        let state_style = base;
        let cwd = clip(&s.cwd, (inside.width as usize).saturating_sub(42));
        let spans = vec![
            Span::styled(format!(" {name:<18} "), base),
            Span::styled(format!(" {state_text:<8} "), state_style),
            Span::styled(format!(" {:>2} tabs ", s.tabs.len()), base),
            Span::styled(cwd, base),
        ];
        f.render_widget(
            Paragraph::new(Line::from(spans)).style(base),
            line_rect,
        );
    }

    if let Some(msg) = state.message.as_deref() {
        let msg_y = inside.y + inside.height.saturating_sub(2);
        f.render_widget(
            Paragraph::new(Span::styled(
                format!(" {msg}"),
                theme::dialog_bg().fg(theme::FG_BRIGHT),
            )),
            Rect::new(inside.x, msg_y, inside.width, 1),
        );
    }

    let hint_y = inside.y + inside.height.saturating_sub(1);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Enter", theme::key_hint()),
            Span::styled(" attach    ", theme::dialog_muted()),
            Span::styled("Esc", theme::dialog_bg().fg(theme::FG)),
            Span::styled(" cancel", theme::dialog_muted()),
        ]))
        .style(theme::dialog_bg()),
        Rect::new(inside.x, hint_y, inside.width, 1),
    )
}

fn clip(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else if max == 1 {
        "…".into()
    } else {
        let mut out: String = chars.into_iter().take(max - 1).collect();
        out.push('…');
        out
    }
}
