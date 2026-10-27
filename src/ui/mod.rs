//! Frame composition and permanent chrome.
//!
//! One row of chrome: the tab strip. There is no status line and no frame —
//! the terminal below the strip is the window.

pub mod dialogs;
pub mod header;
pub mod pane;
pub mod theme;

use crate::app::{App, Mode};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Block;

#[derive(Debug, Clone, Copy, Default)]
pub struct Layout {
    pub header: Rect,
    pub term: Rect,
}

/// The one geometry every renderer, mouse handler and PTY resize goes through.
///
/// One row of chrome: the tab strip. The terminal gets everything below it,
/// full width. There is no status line and no scrollbar column.
pub fn layout(area: Rect) -> Layout {
    let header = Rect::new(area.x, area.y, area.width, area.height.min(1));
    let top = area.y + header.height;
    Layout {
        header,
        term: Rect::new(
            area.x,
            top,
            area.width,
            area.height.saturating_sub(header.height),
        ),
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    f.render_widget(Block::new().style(theme::base()), area);
    app.regions.clear();

    let l = layout(area);
    if area.height > 0 {
        header::render(f, l.header, app);
    }
    if l.term.width > 0 && l.term.height > 0 {
        pane::render(f, app, l);
    }

    match app.mode {
        Mode::TabPicker => dialogs::tab_picker(f, area, app),
        Mode::Help => dialogs::help(f, area, app),
        Mode::Normal => {}
    }
}
