//! The terminal pane. The pane keeps the terminal's own default colours.

use crate::app::{App, Mode};
use crate::ui::Layout;
use ratatui::Frame;
use ratatui::layout::Position;
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
    // No background style is applied to the widget: vt100 cells with default
    // colours render as `Reset`, so the terminal's own background and
    // foreground show through unchanged.
    f.render_widget(
        PseudoTerminal::new(parser.screen()).cursor(cursor),
        l.term,
    );
    drop(parser);
    if show_cursor {
        let (row, col) = cursor_pos;
        if col < l.term.width && row < l.term.height {
            f.set_cursor_position(Position::new(l.term.x + col, l.term.y + row));
        }
    }
}

