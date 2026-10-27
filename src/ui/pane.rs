//! The terminal pane, its default colours, and the mouse-selection overlay.

use crate::app::{App, Mode, Selection};
use crate::ui::{Layout, theme};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Style};
use tui_term::widget::{Cursor, PseudoTerminal};

pub fn render(f: &mut Frame, app: &mut App, l: Layout) {
    let Some(tab) = app.tabs.get(app.active) else {
        return;
    };
    app.regions.pane = Some(l.term);

    let Ok(parser) = tab.session.parser().lock() else {
        return;
    };
    let scrolling = parser.screen().scrollback() > 0;
    // tui-term draws a block; use the real terminal cursor instead so its
    // shape and blink follow the outer terminal. Ratatui hides the cursor
    // unless we ask for it.
    let cursor_pos = parser.screen().cursor_position();
    let show_cursor = app.mode == Mode::Normal
        && !scrolling
        && !tab.session.has_exited()
        && !parser.screen().hide_cursor();
    let mut cursor = Cursor::default();
    cursor.hide();
    // vt100 reports default colours as `Reset`, which would leave the canvas
    // to the outer terminal's palette. Draw normally, then replace only those
    // resets.
    f.render_widget(
        PseudoTerminal::new(parser.screen())
            .cursor(cursor)
            .style(Style::new().bg(theme::BLACK).fg(theme::GRAY)),
        l.term,
    );
    drop(parser);
    apply_defaults(f, l.term);
    if show_cursor {
        let (row, col) = cursor_pos;
        if col < l.term.width && row < l.term.height {
            f.set_cursor_position(Position::new(l.term.x + col, l.term.y + row));
        }
    }

    if let Some(sel) = app.selection {
        paint_selection(f, l.term, sel);
    }
}

/// vt100 leaves default foreground/background as `Reset`; give the pane the
/// Turbo Vision canvas without touching colours a program chose itself.
fn apply_defaults(f: &mut Frame, term: Rect) {
    let buf = f.buffer_mut();
    for row in 0..term.height {
        for col in 0..term.width {
            if let Some(cell) = buf.cell_mut(Position::new(term.x + col, term.y + row)) {
                if cell.fg == Color::Reset {
                    cell.fg = theme::GRAY;
                }
                if cell.bg == Color::Reset {
                    cell.bg = theme::BLACK;
                }
            }
        }
    }
}

fn paint_selection(f: &mut Frame, term: Rect, sel: Selection) {
    if sel.is_empty() {
        return;
    }
    let ((sr, sc), (er, ec)) = sel.normalized();
    let buf = f.buffer_mut();
    for row in sr..=er {
        if row >= term.height {
            break;
        }
        let c0 = if row == sr { sc } else { 0 };
        let c1 = if row == er { ec } else { term.width.saturating_sub(1) };
        for col in c0..=c1 {
            if col >= term.width {
                break;
            }
            if let Some(cell) = buf.cell_mut(Position::new(term.x + col, term.y + row)) {
                cell.set_style(theme::selection());
            }
        }
    }
}
