//! Turbo Vision modal dialogs: the tab list and help.

use crate::app::{App, DialogButton, HoverTarget};
use crate::ui::theme;
use ratatui::Frame;
use ratatui::layout::{Alignment, Position, Rect};
use ratatui::style::Style;
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub fn dialog(f: &mut Frame, area: Rect, title: &str) {
    // A hard black shadow, one cell right and down, as Turbo Vision drew it.
    let shadow = Rect::new(
        area.x.saturating_add(1).min(f.area().right().saturating_sub(1)),
        area.y.saturating_add(1).min(f.area().bottom().saturating_sub(1)),
        area.width.min(f.area().right().saturating_sub(area.x.saturating_add(1))),
        area.height.min(f.area().bottom().saturating_sub(area.y.saturating_add(1))),
    );
    f.render_widget(Clear, shadow);
    f.render_widget(Block::new().style(theme::dialog_shadow()), shadow);
    f.render_widget(Clear, area);
    f.render_widget(
        Block::new()
            .borders(Borders::ALL)
            .border_set(border::DOUBLE)
            .border_style(theme::dialog_frame())
            .style(theme::dialog_bg())
            .title(
                Line::from(Span::styled(
                    format!(" {title} "),
                    theme::dialog_title(),
                ))
                .alignment(Alignment::Center),
            ),
        area,
    );
}

pub fn inner(area: Rect) -> Rect {
    Rect::new(
        area.x + 2,
        area.y + 1,
        area.width.saturating_sub(4),
        area.height.saturating_sub(2),
    )
}

pub fn tab_picker(f: &mut Frame, screen: Rect, app: &mut App) {
    let n = app.tabs.len();
    let w = 58.min(screen.width.saturating_sub(2)).max(24.min(screen.width));
    let h = (n as u16 + 6)
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
    dialog(f, rect, "Select Tab");
    let inside = inner(rect);
    let rows = inside.height.saturating_sub(2) as usize;
    if rows == 0 {
        return;
    }
    let titles: Vec<(String, bool)> = app
        .tabs
        .iter()
        .enumerate()
        .map(|(i, t)| (t.title.clone(), i == app.active))
        .collect();
    let cursor = app.tab_picker.as_ref().map(|p| p.cursor).unwrap_or(0).min(n.saturating_sub(1));
    let scroll = {
        let p = app.tab_picker.as_mut().expect("picker open");
        if cursor < p.scroll {
            p.scroll = cursor;
        }
        if cursor >= p.scroll + rows {
            p.scroll = cursor + 1 - rows;
        }
        p.scroll.min(n.saturating_sub(1))
    };
    for (row, i) in (scroll..n.min(scroll + rows)).enumerate() {
        let y = inside.y + row as u16;
        let line_rect = Rect::new(inside.x, y, inside.width, 1);
        app.regions.dialog_rows.push((line_rect, i));
        let hovered = app.hover == HoverTarget::DialogRow(i);
        let selected = i == cursor;
        let row_style = if selected {
            theme::list_selected()
        } else if hovered {
            theme::list_hover()
        } else {
            theme::dialog_bg()
        };
        let (title, active) = &titles[i];
        let avail = inside.width.saturating_sub(8) as usize;
        let title = clip(title, avail);
        let mut spans = vec![
            Span::styled(format!(" {:>2}  ", i + 1), row_style.fg(theme::RED)),
            Span::styled(title, row_style),
        ];
        if *active {
            spans.push(Span::styled("  active", row_style.fg(theme::GREEN)));
        }
        f.render_widget(
            Paragraph::new(Line::from(spans)).style(row_style),
            line_rect,
        );
    }

    let by = inside.y + inside.height.saturating_sub(1);
    let labels = [
        ("Go To", DialogButton::GoTo, true),
        ("Cancel", DialogButton::Cancel, false),
    ];
    let mut x = inside.x + 2;
    for (label, button, default) in labels {
        let text = format!("[ {label} ]");
        let rect = Rect::new(x, by, text.chars().count() as u16, 1);
        app.regions.dialog_buttons.push((rect, button));
        let style = if app.hover == HoverTarget::DialogButton(button) {
            theme::button_hover()
        } else if default {
            theme::button_default()
        } else {
            theme::button()
        };
        f.render_widget(Paragraph::new(Span::styled(text, style)), rect);
        x += rect.width + 3;
    }
}

pub fn help(f: &mut Frame, screen: Rect, app: &mut App) {
    let w = 62.min(screen.width.saturating_sub(2)).max(30.min(screen.width));
    let h = 17.min(screen.height.saturating_sub(2)).max(9.min(screen.height));
    if w == 0 || h == 0 {
        return;
    }
    let rect = Rect::new(
        screen.x + (screen.width.saturating_sub(w)) / 2,
        screen.y + (screen.height.saturating_sub(h)) / 2,
        w,
        h,
    );
    dialog(f, rect, "Help");
    let inside = inner(rect);
    let rows = [
        ("Ctrl+A C", "Open a new tab"),
        ("Ctrl+A D", "Detach from this session"),
        ("Ctrl+A \"", "Select a tab from a list"),
        ("Ctrl+A N / P", "Next / previous tab"),
        ("Ctrl+A K", "Close the current tab"),
        ("Alt+← / →", "Next / previous tab"),
        ("Alt+1…9", "Jump to a tab"),
        ("Ctrl+A A", "Send a literal Ctrl+A"),
        ("Mouse", "Click tabs, +, ×; wheel scrolls"),
    ];
    for (i, (key, text)) in rows.iter().enumerate() {
        if i as u16 >= inside.height.saturating_sub(2) {
            break;
        }
        let line = Line::from(vec![
            Span::styled(format!(" {:<12}", key), theme::status_value().bg(theme::GRAY)),
            Span::styled(*text, Style::new().bg(theme::GRAY).fg(theme::BLACK)),
        ]);
        f.render_widget(
            Paragraph::new(line),
            Rect::new(inside.x, inside.y + i as u16, inside.width, 1),
        );
    }
    let button = DialogButton::Ok;
    let label = "[ OK ]";
    let by = inside.y + inside.height.saturating_sub(1);
    let x = inside.x + (inside.width.saturating_sub(label.len() as u16)) / 2;
    let brect = Rect::new(x, by, label.len() as u16, 1);
    app.regions.dialog_buttons.push((brect, button));
    let style = if app.hover == HoverTarget::DialogButton(button) {
        theme::button_hover()
    } else {
        theme::button_default()
    };
    f.render_widget(Paragraph::new(Span::styled(label, style)), brect);
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

#[allow(dead_code)]
fn _position(x: u16, y: u16) -> Position {
    Position::new(x, y)
}
