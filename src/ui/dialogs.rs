//! Borderless modal dialogs: the tab list and help.

use crate::app::{App, HoverTarget};
use crate::ui::theme;
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

pub fn dialog(f: &mut Frame, area: Rect, title: &str) {
    f.render_widget(Clear, area);
    f.render_widget(Block::new().style(theme::dialog_bg()), area);
    f.render_widget(
        Paragraph::new(Span::styled(title, theme::dialog_title()))
            .style(theme::dialog_bg()),
        Rect::new(
            area.x + 2,
            area.y + 1,
            area.width.saturating_sub(4),
            1,
        ),
    );
}

/// Content area inside a borderless dialog: one blank row above the title, the
/// title, one blank row, the rows, and the hint on the last row. The last row
/// of `inner` is the hint, so the body ends one row above the dialog's bottom.
pub fn inner(area: Rect) -> Rect {
    Rect::new(
        area.x + 2,
        area.y + 2,
        area.width.saturating_sub(4),
        area.height.saturating_sub(2),
    )
}

pub fn tab_picker(f: &mut Frame, screen: Rect, app: &mut App) {
    let n = app.tabs.len();
    let titles: Vec<(String, bool)> = app
        .tabs
        .iter()
        .enumerate()
        .map(|(i, t)| (t.title(), i == app.active))
        .collect();
    // Room for the longest name, hand-set or program-set, within the screen.
    // The tab strip clips at MAX_TITLE; the picker shows it whole.
    let longest = titles
        .iter()
        .map(|(title, _)| title.chars().count())
        .max()
        .unwrap_or(0)
        .min(usize::from(u16::MAX)) as u16;
    let w = longest
        .saturating_add(n.to_string().len() as u16)
        .saturating_add(8)
        .max(58)
        .min(screen.width.saturating_sub(2))
        .max(24.min(screen.width));
    let h = (n as u16 + 6)
        .min(screen.height.saturating_sub(2))
        .max(7.min(screen.height));
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
    let rows = inside.height.saturating_sub(1) as usize;
    if rows == 0 {
        return;
    }
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
    let rename = app.tab_picker.as_ref().and_then(|p| p.rename.as_ref());
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
        let number = i + 1;
        let digits = number.to_string().len() as u16;
        let avail = inside.width.saturating_sub(4 + digits) as usize;
        let editing = rename.is_some() && i == cursor;
        let (title, cursor_col) = match rename {
            Some(r) if editing => rename_window(&r.text, r.cursor, avail),
            _ => (clip(title, avail), 0),
        };
        // The tab that is showing is marked with a filled circle in a gutter,
        // the way OpenCode's model picker marks the current choice.
        let marker = if *active { "●" } else { " " };
        // Named the way the tab strip names it: "1:bash", not a bare number.
        let spans = vec![
            Span::styled(format!(" {marker} "), row_style),
            Span::styled(format!("{number}:{title}"), row_style),
        ];
        f.render_widget(
            Paragraph::new(Line::from(spans)).style(row_style),
            line_rect,
        );
        if editing {
            let x = inside.x + 4 + digits + cursor_col;
            f.set_cursor_position(Position::new(x, y));
        }
    }

    let hint_y = inside.y + inside.height.saturating_sub(2);
    let mut hints = vec![
        Span::styled("Enter", theme::key_hint()),
        Span::styled(
            if rename.is_some() { " save    " } else { " goto    " },
            theme::dialog_muted(),
        ),
    ];
    if rename.is_none() {
        hints.push(Span::styled("e", theme::dialog_bg().fg(theme::FG)));
        hints.push(Span::styled(" rename    ", theme::dialog_muted()));
    }
    hints.push(Span::styled("Esc", theme::dialog_bg().fg(theme::FG)));
    hints.push(Span::styled(" cancel", theme::dialog_muted()));
    f.render_widget(
        Paragraph::new(Line::from(hints)).style(theme::dialog_bg()),
        Rect::new(inside.x, hint_y, inside.width, 1),
    )
}

/// A one-line window over the rename buffer that keeps the insertion point
/// visible when the name is wider than the dialog.
fn rename_window(text: &str, cursor: usize, width: usize) -> (String, u16) {
    if width == 0 {
        return (String::new(), 0);
    }
    let chars: Vec<char> = text.chars().collect();
    let cursor = cursor.min(chars.len());
    let start = cursor.saturating_sub(width - 1);
    let shown: String = chars[start..].iter().take(width).collect();
    (shown, (cursor - start) as u16)
}

pub fn help(f: &mut Frame, screen: Rect, _app: &mut App) {
    let w = 62.min(screen.width.saturating_sub(2)).max(30.min(screen.width));
    let h = 14.min(screen.height.saturating_sub(2)).max(10.min(screen.height));
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
        ("e", "Rename the selected tab (empty clears)"),
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
            Span::styled(format!(" {:<14}", key), theme::key_hint()),
            Span::styled(*text, theme::dialog_bg()),
        ]);
        f.render_widget(
            Paragraph::new(line).style(theme::dialog_bg()),
            Rect::new(inside.x, inside.y + i as u16, inside.width, 1),
        );
    }
    let hint_y = inside.y + inside.height.saturating_sub(2);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Enter", theme::key_hint()),
            Span::styled(" close    ", theme::dialog_muted()),
            Span::styled("Esc", theme::dialog_bg().fg(theme::FG)),
            Span::styled(" close", theme::dialog_muted()),
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
