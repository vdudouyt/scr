//! The bottom Turbo Vision status line.

use crate::app::{App, Mode};
use crate::ui::theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 {
        return;
    }
    let right = format!(
        " {} · {} tab{} ",
        app.name,
        app.tabs.len(),
        if app.tabs.len() == 1 { "" } else { "s" }
    );
    let left = hints(app);
    let right_width = right.chars().count();
    let mut spans: Vec<Span> = Vec::new();

    if app.prefix && app.mode == Mode::Normal {
        let text = " ^A  C New  D Detach  \" Tabs  ? Help ";
        let used = text.chars().count().min(area.width as usize);
        spans.push(Span::styled(text.to_string(), theme::prefix_bar()));
        spans.push(Span::styled(
            " ".repeat((area.width as usize).saturating_sub(used)),
            theme::statusbar(),
        ));
        f.render_widget(Paragraph::new(Line::from(spans)), area);
        return;
    }

    let mut text: Vec<(String, ratatui::style::Style)> = Vec::new();
    if let Some(flash) = app.flash_text() {
        text.push((format!(" {flash} "), theme::status_value()));
        text.push(("  ".into(), theme::statusbar()));
    }
    text.extend(left);

    let mut used: usize = text.iter().map(|(s, _)| s.chars().count()).sum();
    if used + right_width > area.width as usize {
        // Drop the hints before the session identity, and the flash first.
        while used + right_width > area.width as usize && !text.is_empty() {
            let (last, _) = text.pop().unwrap();
            used = used.saturating_sub(last.chars().count());
        }
    }
    if used + right_width <= area.width as usize {
        let gap = area.width as usize - used - right_width;
        spans.extend(text.into_iter().map(|(s, st)| Span::styled(s, st)));
        spans.push(Span::styled(" ".repeat(gap), theme::statusbar()));
        spans.push(Span::styled(right, theme::statusbar()));
    } else {
        // Extremely narrow: just the right-hand identity, clipped by ratatui.
        spans.push(Span::styled(right, theme::statusbar()));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn hints(app: &App) -> Vec<(String, ratatui::style::Style)> {
    let mut v = Vec::new();
    let pairs = [
        ("^A C", "New"),
        ("^A D", "Detach"),
        ("^A \"", "Tabs"),
        ("^A ?", "Help"),
    ];
    for (k, label) in pairs {
        v.push((format!(" {k} "), theme::status_key()));
        v.push((format!("{label}  "), theme::statusbar()));
    }
    let _ = app;
    v
}
