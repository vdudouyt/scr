//! A ratatui backend that writes ANSI into a socket buffer.
//!
//! This deliberately wraps `CrosstermBackend`, the same backend OpenAdmin uses.
//! The only differences are the sink (a buffer or stdout instead of a real tty)
//! and the size/cursor queries, which the daemon answers from the attached
//! client rather than from its own (non-existent) controlling terminal.

use ratatui::backend::{Backend, ClearType, CrosstermBackend, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

enum Sink {
    Buffer(Arc<Mutex<Vec<u8>>>),
    Stdout,
}

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Sink::Buffer(out) => {
                out.lock()
                    .expect("frame buffer lock")
                    .extend_from_slice(buf);
                Ok(buf.len())
            }
            Sink::Stdout => std::io::stdout().write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Sink::Buffer(_) => Ok(()),
            Sink::Stdout => std::io::stdout().flush(),
        }
    }
}

pub struct AnsiBackend {
    inner: CrosstermBackend<Sink>,
    buffer: Option<Arc<Mutex<Vec<u8>>>>,
    size: Size,
    cursor: Position,
    cursor_visible: bool,
}

impl AnsiBackend {
    pub fn new(width: u16, height: u16) -> Self {
        let buffer = Arc::new(Mutex::new(Vec::with_capacity(16 * 1024)));
        AnsiBackend {
            inner: CrosstermBackend::new(Sink::Buffer(Arc::clone(&buffer))),
            buffer: Some(buffer),
            size: Size::new(width.max(1), height.max(1)),
            cursor: Position::new(0, 0),
            // The client hides the hardware cursor on entering the alternate
            // screen. Start from that state so the first requested cursor
            // actually emits ?25h.
            cursor_visible: false,
        }
    }

    /// A backend that also writes itself to stdout, for the client-side
    /// session picker.
    pub fn stdout(width: u16, height: u16) -> Self {
        let mut b = Self::new(width, height);
        b.inner = CrosstermBackend::new(Sink::Stdout);
        b.buffer = None;
        b
    }

    pub fn take(&mut self) -> Vec<u8> {
        match &self.buffer {
            Some(buf) => {
                let mut guard = buf.lock().expect("frame buffer lock");
                std::mem::take(&mut *guard)
            }
            None => Vec::new(),
        }
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        self.size = Size::new(width.max(1), height.max(1));
    }
}

impl Backend for AnsiBackend {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        self.inner.draw(content)
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        if self.cursor_visible {
            self.inner.hide_cursor()?;
            self.cursor_visible = false;
        }
        Ok(())
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        if !self.cursor_visible {
            self.inner.show_cursor()?;
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
        self.inner.set_cursor_position(p)
    }

    fn clear(&mut self) -> io::Result<()> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        self.inner.clear_region(clear_type)
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
        ratatui::backend::Backend::flush(&mut self.inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Style};

    #[test]
    fn a_cell_becomes_a_cursor_position_style_and_symbol() {
        let mut b = AnsiBackend::new(80, 24);
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        buf[(0, 0)].set_symbol("A");
        b.draw(buf.content.iter().take(1).enumerate().map(|(i, c)| {
            (i as u16, 0, c)
        }))
        .unwrap();
        let out = String::from_utf8(b.take()).unwrap();
        assert!(out.contains("\x1b[1;1H"));
        assert!(out.contains('A'));
    }

    #[test]
    fn colors_use_truecolour() {
        let mut b = AnsiBackend::new(10, 2);
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        buf[(0, 0)].set_symbol("x").set_style(
            Style::new()
                .fg(Color::Rgb(1, 2, 3))
                .bg(Color::Rgb(4, 5, 6)),
        );
        b.draw(buf.content.iter().take(1).enumerate().map(|(i, c)| {
            (i as u16, 0, c)
        }))
        .unwrap();
        let out = String::from_utf8(b.take()).unwrap();
        assert!(out.contains("38;2;1;2;3"));
        assert!(out.contains("48;2;4;5;6"));
    }

    #[test]
    fn adjacent_same_style_cells_share_one_move() {
        let mut b = AnsiBackend::new(20, 2);
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        for x in 0..5 {
            buf[(x, 0)]
                .set_symbol("x")
                .set_style(Style::new().fg(Color::Red).bg(Color::Blue));
        }
        b.draw(buf.content.iter().enumerate().map(|(i, c)| {
            (i as u16, 0, c)
        }))
        .unwrap();
        let out = String::from_utf8(b.take()).unwrap();
        assert_eq!(out.matches("\x1b[1;1H").count(), 1);
        assert!(!out.contains("\x1b[1;2H"));
        assert_eq!(out.matches('x').count(), 5);
    }

    #[test]
    fn a_frame_starts_and_ends_with_known_cursor_state() {
        let mut b = AnsiBackend::new(10, 2);
        assert!(!b.cursor_visible);
        b.show_cursor().unwrap();
        assert!(b.cursor_visible);
        b.hide_cursor().unwrap();
        assert!(!b.cursor_visible);
    }
}
