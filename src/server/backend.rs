//! A ratatui backend that renders into an ANSI byte stream instead of stdout.
//!
//! The session daemon has no terminal of its own; the attached client has the
//! terminal. Frames are written to the socket, and the client relays them
//! verbatim. Ratatui still computes cell diffs, so only changed cells travel.

use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};
use ratatui::style::{Color, Modifier};
use std::io;
use std::io::Write;

pub struct AnsiBackend {
    out: Vec<u8>,
    size: Size,
    cursor: Position,
    cursor_visible: bool,
    sink: Option<Box<dyn Write + Send>>,
}

impl AnsiBackend {
    pub fn new(width: u16, height: u16) -> Self {
        AnsiBackend {
            out: Vec::with_capacity(16 * 1024),
            size: Size::new(width.max(1), height.max(1)),
            cursor: Position::new(0, 0),
            // The client hides the hardware cursor when it enters the
            // alternate screen. Start from that state so the first frame that
            // asks for a cursor actually emits ?25h.
            cursor_visible: false,
            sink: None,
        }
    }

    /// A backend that also writes each flushed frame to stdout. Used by the
    /// session picker, which lives in the client and has no socket to send to.
    pub fn stdout(width: u16, height: u16) -> Self {
        let mut b = Self::new(width, height);
        b.sink = Some(Box::new(std::io::stdout()));
        b
    }

    pub fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.out)
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        self.size = Size::new(width.max(1), height.max(1));
    }

    fn move_to(&mut self, x: u16, y: u16) {
        let _ = write!(self.out, "\x1b[{};{}H", y + 1, x + 1);
    }
}

impl Backend for AnsiBackend {
    type Error = io::Error;

    #[allow(deprecated)]
    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        for (x, y, cell) in content {
            if cell.skip {
                continue;
            }
            self.move_to(x, y);
            self.out.extend_from_slice(b"\x1b[0m");
            let mut mods = String::new();
            let m = cell.modifier;
            for (flag, code) in [
                (Modifier::BOLD, "1"),
                (Modifier::DIM, "2"),
                (Modifier::ITALIC, "3"),
                (Modifier::UNDERLINED, "4"),
                (Modifier::SLOW_BLINK, "5"),
                (Modifier::RAPID_BLINK, "6"),
                (Modifier::REVERSED, "7"),
                (Modifier::HIDDEN, "8"),
                (Modifier::CROSSED_OUT, "9"),
            ] {
                if m.contains(flag) {
                    mods.push_str(code);
                    mods.push(';');
                }
            }
            if !mods.is_empty() {
                mods.pop();
                let _ = write!(self.out, "\x1b[{mods}m");
            }
            push_color(&mut self.out, cell.fg, true);
            push_color(&mut self.out, cell.bg, false);
            if cell.underline_color != Color::Reset {
                push_underline(&mut self.out, cell.underline_color);
            }
            self.out.extend_from_slice(cell.symbol().as_bytes());
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        if self.cursor_visible {
            self.out.extend_from_slice(b"\x1b[?25l");
            self.cursor_visible = false;
        }
        Ok(())
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        if !self.cursor_visible {
            self.out.extend_from_slice(b"\x1b[?25h");
            self.cursor_visible = true;
        }
        Ok(())
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(self.cursor)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        let p = position.into();
        self.cursor = p;
        self.move_to(p.x, p.y);
        Ok(())
    }

    fn clear(&mut self) -> io::Result<()> {
        self.out.extend_from_slice(b"\x1b[2J\x1b[H");
        self.cursor = Position::new(0, 0);
        Ok(())
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        let seq: &[u8] = match clear_type {
            ClearType::All => b"\x1b[2J",
            ClearType::AfterCursor => b"\x1b[0J",
            ClearType::BeforeCursor => b"\x1b[1J",
            ClearType::CurrentLine => b"\x1b[2K",
            ClearType::UntilNewLine => b"\x1b[0K",
        };
        self.out.extend_from_slice(seq);
        Ok(())
    }

    fn size(&self) -> io::Result<Size> {
        Ok(self.size)
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        Ok(WindowSize {
            columns_rows: self.size,
            pixels: Size::new(0, 0),
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(sink) = self.sink.as_mut() {
            let out = std::mem::take(&mut self.out);
            sink.write_all(&out)?;
            sink.flush()?;
        }
        Ok(())
    }

}


fn push_color(out: &mut Vec<u8>, color: Color, fg: bool) {
    let (base, bright, ext) = if fg {
        (30, 90, 38)
    } else {
        (40, 100, 48)
    };
    match color {
        Color::Reset => {
            let _ = write!(out, "\x1b[{}m", if fg { 39 } else { 49 });
        }
        Color::Black => {
            let _ = write!(out, "\x1b[{}m", base);
        }
        Color::Red => {
            let _ = write!(out, "\x1b[{}m", base + 1);
        }
        Color::Green => {
            let _ = write!(out, "\x1b[{}m", base + 2);
        }
        Color::Yellow => {
            let _ = write!(out, "\x1b[{}m", base + 3);
        }
        Color::Blue => {
            let _ = write!(out, "\x1b[{}m", base + 4);
        }
        Color::Magenta => {
            let _ = write!(out, "\x1b[{}m", base + 5);
        }
        Color::Cyan => {
            let _ = write!(out, "\x1b[{}m", base + 6);
        }
        Color::Gray => {
            let _ = write!(out, "\x1b[{}m", base + 7);
        }
        Color::DarkGray => {
            let _ = write!(out, "\x1b[{}m", bright);
        }
        Color::LightRed => {
            let _ = write!(out, "\x1b[{}m", bright + 1);
        }
        Color::LightGreen => {
            let _ = write!(out, "\x1b[{}m", bright + 2);
        }
        Color::LightYellow => {
            let _ = write!(out, "\x1b[{}m", bright + 3);
        }
        Color::LightBlue => {
            let _ = write!(out, "\x1b[{}m", bright + 4);
        }
        Color::LightMagenta => {
            let _ = write!(out, "\x1b[{}m", bright + 5);
        }
        Color::LightCyan => {
            let _ = write!(out, "\x1b[{}m", bright + 6);
        }
        Color::White => {
            let _ = write!(out, "\x1b[{}m", bright + 7);
        }
        Color::Rgb(r, g, b) => {
            let _ = write!(out, "\x1b[{ext};2;{r};{g};{b}m");
        }
        Color::Indexed(n) => {
            let _ = write!(out, "\x1b[{ext};5;{n}m");
        }
    }
}

fn push_underline(out: &mut Vec<u8>, color: Color) {
    match color {
        Color::Rgb(r, g, b) => {
            let _ = write!(out, "\x1b[58;2;{r};{g};{b}m");
        }
        Color::Indexed(n) => {
            let _ = write!(out, "\x1b[58;5;{n}m");
        }
        Color::Reset => {}
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    #[test]
    fn a_cell_becomes_cursor_position_style_and_symbol() {
        let mut b = AnsiBackend::new(80, 24);
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        buf[(0, 0)].set_symbol("A");
        b.draw(buf.content.iter().take(1).enumerate().map(|(i, c)| {
            (i as u16, 0, c)
        }))
        .unwrap();
        let out = String::from_utf8(b.take()).unwrap();
        assert!(out.contains("\x1b[1;1H"));
        assert!(out.ends_with('A'));
    }

    #[test]
    fn colors_use_truecolour() {
        let mut b = AnsiBackend::new(10, 2);
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        buf[(0, 0)]
            .set_symbol("x")
            .set_style(ratatui::style::Style::new().fg(Color::Rgb(1, 2, 3)).bg(Color::Rgb(4, 5, 6)));
        b.draw(buf.content.iter().enumerate().map(|(i, c)| {
            (i as u16, 0, c)
        }))
        .unwrap();
        let out = String::from_utf8(b.take()).unwrap();
        assert!(out.contains("38;2;1;2;3"));
        assert!(out.contains("48;2;4;5;6"));
    }
}
