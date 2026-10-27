//! Frame composition and permanent chrome.
//!
//! Two rows of chrome: the tab strip on top and the Turbo Vision status line at
//! the bottom. No window frame — the terminal is the window.

pub mod dialogs;
pub mod header;
pub mod pane;
pub mod status;
pub mod theme;

use crate::app::{App, Mode};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Block;

#[derive(Debug, Clone, Copy, Default)]
pub struct Layout {
    pub header: Rect,
    pub term: Rect,
    pub bar: Rect,
    pub status: Option<Rect>,
}

/// The one geometry every renderer, mouse handler and PTY resize goes through.
///
/// Rows: tab strip (1), terminal, status line (1 when there is room for it).
/// Columns: terminal, then one scrollbar column. With no frame border there is
/// nothing else to reserve.
pub fn layout(area: Rect) -> Layout {
    let header = Rect::new(area.x, area.y, area.width, area.height.min(1));
    let status = (area.height >= 3).then(|| {
        Rect::new(
            area.x,
            area.y + area.height - 1,
            area.width,
            1,
        )
    });
    let top = area.y + header.height;
    let bottom = status.map_or(area.y + area.height, |r| r.y);
    let term_h = bottom.saturating_sub(top);
    let bar_w = u16::from(area.width > 1);
    let term = Rect::new(
        area.x,
        top,
        area.width.saturating_sub(bar_w),
        term_h,
    );
    let bar = if bar_w > 0 && term_h > 0 {
        Rect::new(area.x + area.width - 1, top, 1, term_h)
    } else {
        Rect::default()
    };
    Layout {
        header,
        term,
        bar,
        status,
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
    if let Some(status) = l.status {
        status::render(f, status, app);
    }

    match app.mode {
        Mode::TabPicker => dialogs::tab_picker(f, area, app),
        Mode::Help => dialogs::help(f, area, app),
        Mode::Normal => {}
    }
}
