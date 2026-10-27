//! The terminal pane, its TV scrollbar, and the mouse-selection overlay.

use crate::app::{App, HoverTarget, Mode, Selection};
use crate::term::scrollback::{self, History};
use crate::ui::{Layout, theme};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use tui_term::widget::{Cursor, PseudoTerminal};

pub fn render(f: &mut Frame, app: &mut App, l: Layout) {
    let Some(tab) = app.tabs.get(app.active) else {
        return;
    };
    app.regions.pane = Some(l.term);
    if l.bar.width > 0 {
        app.regions.pane_bar = Some(l.bar);
    }

    let Ok(mut parser) = tab.session.parser().lock() else {
        return;
    };
    let history = scrollback::history(parser.screen_mut());
    let scrolling = parser.screen().scrollback() > 0;
    // tui-term draws a block; use the real terminal cursor instead so its
    // shape and blink follow the outer terminal, and so it cannot disappear
    // into the canvas. Ratatui hides the cursor unless we ask for it.
    let cursor_pos = parser.screen().cursor_position();
    let show_cursor = app.mode == Mode::Normal
        && !scrolling
        && !tab.session.has_exited()
        && !parser.screen().hide_cursor();
    let mut cursor = Cursor::default();
    cursor.hide();
    // vt100 reports default colours as `Reset`, which would leave the canvas
    // to the outer terminal's palette. Draw normally, then replace only those
    // resets. This is much cheaper than copying every cell into a wrapper (and
    // keeps full-screen programs responsive).
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

    if l.bar.width > 0 && l.bar.height > 0 {
        render_bar(f, l.bar, history, l.term.height, app.hover == HoverTarget::Scrollbar);
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

fn render_bar(f: &mut Frame, area: Rect, history: Option<History>, term_rows: u16, hot: bool) {
    let arrow = if hot {
        theme::scroll_thumb()
    } else {
        Style::new().bg(theme::BLACK).fg(theme::GRAY)
    };
    if area.height < 3 {
        f.render_widget(
            Paragraph::new(Span::styled("│", theme::scroll_track())),
            area,
        );
        return;
    }
    let track_rows = area.height - 2;
    let mut lines: Vec<Line> = Vec::with_capacity(area.height as usize);
    lines.push(Line::from(Span::styled("▲", arrow)));
    let Some(h) = history else {
        for _ in 0..track_rows {
            lines.push(Line::from(Span::styled("│", theme::scroll_track())));
        }
        lines.push(Line::from(Span::styled("▼", arrow)));
        f.render_widget(Paragraph::new(lines), area);
        return;
    };
    let g = ScrollGeometry {
        track: track_rows as usize,
        max: h.lines,
        viewport: term_rows as usize,
    };
    let (start, len) = g.thumb(h.lines - h.offset);
    let thumb_style = theme::scroll_thumb().add_modifier(Modifier::BOLD);
    for i in 0..track_rows as usize {
        if i >= start && i < start + len {
            lines.push(Line::from(Span::styled("█", thumb_style)));
        } else {
            lines.push(Line::from(Span::styled("│", theme::scroll_track())));
        }
    }
    lines.push(Line::from(Span::styled("▼", arrow)));
    f.render_widget(Paragraph::new(lines), area);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollGeometry {
    pub track: usize,
    pub max: usize,
    pub viewport: usize,
}

impl ScrollGeometry {
    pub fn thumb(&self, position: usize) -> (usize, usize) {
        let round = |n: usize, d: usize| (n + d / 2) / d;
        if self.track == 0 {
            return (0, 0);
        }
        let whole = self.max + self.viewport;
        if whole == 0 {
            return (0, self.track);
        }
        let len = round(self.viewport * self.track, whole).clamp(1, self.track);
        let start = round(position.min(self.max) * self.track, whole).min(self.track - len);
        (start, len)
    }

    pub fn position_at(&self, row: i32) -> usize {
        let (_, len) = self.thumb(0);
        let travel = self.track.saturating_sub(len);
        if row <= 0 || travel == 0 {
            return 0;
        }
        let row = row as usize;
        if row >= travel {
            return self.max;
        }
        let (mut lo, mut hi) = (0, self.max);
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.thumb(mid).0 >= row {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        if lo > 0 && row - self.thumb(lo - 1).0 < self.thumb(lo).0 - row {
            lo - 1
        } else {
            lo
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_thumb_reaches_both_ends() {
        let g = ScrollGeometry {
            track: 10,
            max: 100,
            viewport: 24,
        };
        assert_eq!(g.thumb(0).0, 0);
        assert_eq!(g.thumb(100).0 + g.thumb(100).1, 10);
        assert_eq!(g.position_at(0), 0);
        assert_eq!(g.position_at(9), 100);
    }
}
