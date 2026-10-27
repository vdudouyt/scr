//! The tab strip: tabs, close buttons and the new-tab button.

use crate::app::{App, HoverTarget};
use crate::ui::theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

const PLUS: u16 = 3;
const MAX_TITLE: usize = 22;

fn digits(n: usize) -> u16 {
    n.to_string().chars().count() as u16
}

fn width_of(n: usize, title: &str) -> u16 {
    // " N:title " + "×"
    digits(n) + title.chars().count() as u16 + 4
}

fn clip_title(title: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let chars: Vec<char> = title.chars().collect();
    if chars.len() <= max {
        return title.to_string();
    }
    if max == 1 {
        return "…".into();
    }
    let mut s: String = chars.into_iter().take(max - 1).collect();
    s.push('…');
    s
}

pub fn render(f: &mut Frame, area: Rect, app: &mut App) {
    if area.width == 0 {
        return;
    }
    let plus_w = if area.width >= 8 { PLUS } else { 0 };
    let tabs_avail = area.width.saturating_sub(plus_w);
    let widths: Vec<u16> = app
        .tabs
        .iter()
        .enumerate()
        .map(|(i, t)| width_of(i + 1, &clip_title(&t.title, MAX_TITLE)))
        .collect();
    let n = widths.len();
    let active = app.active.min(n.saturating_sub(1));
    let (start, end) = if n == 0 || tabs_avail == 0 {
        (0, 0)
    } else {
        window(&widths, active, tabs_avail)
    };
    let mark_l = u16::from(start > 0);
    let mark_r = u16::from(end < n && end > start);

    let mut spans: Vec<Span> = Vec::new();
    let mut x = area.x;
    let tab_right = area.x + tabs_avail;
    if mark_l > 0 {
        spans.push(Span::styled("‹", Style::new().fg(theme::FG_FAINT)));
        x += 1;
    }

    for (i, tab) in app.tabs.iter().enumerate().take(end).skip(start) {
        let max_w = tab_right.saturating_sub(x);
        if max_w < 4 {
            break;
        }
        let mut title = clip_title(&tab.title, MAX_TITLE);
        let mut w = width_of(i + 1, &title);
        if w > max_w {
            let max_title = max_w.saturating_sub(4) as usize;
            title = clip_title(&title, max_title);
            w = width_of(i + 1, &title);
        }
        if w > max_w {
            w = max_w;
        }
        if w < 4 {
            break;
        }
        let rect = Rect::new(x, area.y, w, 1);
        app.regions.tabs.push((rect, i));
        let is_active = i == active;
        let hovered = matches!(app.hover, HoverTarget::Tab(j) | HoverTarget::TabClose(j) if j == i);
        let base = if is_active && hovered {
            theme::tab_active_hover()
        } else if is_active {
            theme::tab_active()
        } else if hovered {
            theme::tab_hover()
        } else {
            theme::tab_idle()
        };

        spans.push(Span::styled(" ", base));
        spans.push(Span::styled(format!("{}", i + 1), base));
        spans.push(Span::styled(":", base));
        spans.push(Span::styled(title, base));
        spans.push(Span::styled(" ", base));
        let close_rect = Rect::new(x + w - 1, area.y, 1, 1);
        app.regions.tab_closes.push((close_rect, i));
        let close_style = if app.hover == HoverTarget::TabClose(i) {
            base.patch(theme::tab_close_hover())
        } else {
            base
        };
        spans.push(Span::styled("×", close_style));
        x += w;
    }

    if mark_r > 0 && x < tab_right {
        spans.push(Span::styled("›", Style::new().fg(theme::FG_FAINT)));
    }

    f.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect::new(area.x, area.y, tabs_avail, 1),
    );

    // The plus follows the last drawn tab when it fits, otherwise it sits at
    // the end of the reserved tab area so it is never pushed off-screen.
    if plus_w > 0 {
        let mut px = x;
        if px + plus_w > tab_right {
            px = tab_right;
        }
        if px + plus_w <= area.x + area.width {
            let rect = Rect::new(px, area.y, plus_w, 1);
            app.regions.new_tab = Some(rect);
            let style = if app.hover == HoverTarget::NewTab {
                theme::tab_hover()
            } else {
                Style::new().bg(theme::BG_BASE).fg(theme::FG_MUTED)
            };
            f.render_widget(Paragraph::new(Span::styled(" + ", style)), rect);
        }
    }
}

fn window(widths: &[u16], active: usize, avail: u16) -> (usize, usize) {
    let n = widths.len();
    let used = |s: usize, e: usize| -> u16 {
        widths[s..e].iter().copied().fold(0u16, u16::saturating_add)
            + u16::from(s > 0)
            + u16::from(e < n)
    };
    let (mut start, mut end) = (active, active + 1);
    loop {
        let mut grew = false;
        if end < n && used(start, end + 1) <= avail {
            end += 1;
            grew = true;
        }
        if start > 0 && used(start - 1, end) <= avail {
            start -= 1;
            grew = true;
        }
        if !grew {
            break;
        }
    }
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::window;

    #[test]
    fn the_active_tab_is_always_in_the_window() {
        let widths = [10u16; 9];
        for active in 0..9 {
            let (s, e) = window(&widths, active, 23);
            assert!(s <= active && active < e);
        }
    }
}
