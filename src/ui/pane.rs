//! The terminal pane and its default colours.

use crate::app::{App, Mode};
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
