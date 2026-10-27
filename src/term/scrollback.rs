//! Scrolling a pane back through its history.
//!
//! vt100 keeps the history — `Config.scrollback` lines per session, the oldest
//! dropped once it is full — and knows how far back the view is. What it does
//! not do is decide when the wheel may move that view, or say how much history
//! there is. Both are here, as functions over the screen rather than methods
//! that lock, so the renderer can use them under the lock it already holds.

/// Move the view `lines` back in time; negative moves toward the live screen.
///
/// Refused where the wheel belongs to the program rather than to us:
/// - on the alternate screen, where a full-screen program is drawing. Nor is
///   the wheel turned into arrow keys there: mc and its like are not scrolled.
/// - when the program asked for mouse reports. The wheel is sent to the
///   program, so the history stays put.
pub fn scroll(screen: &mut vt100::Screen, lines: isize) {
    if screen.alternate_screen() || screen.mouse_protocol_mode() != vt100::MouseProtocolMode::None {
        return;
    }
    // vt100 clamps at the oldest line; the live screen is 0.
    screen.set_scrollback(screen.scrollback().saturating_add_signed(lines));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A five-row screen that has been sent `n` numbered lines. The last line's
    /// newline leaves the cursor on an empty bottom row, so `n - 4` of them
    /// have scrolled into history.
    fn fed(n: usize) -> vt100::Parser {
        let mut p = vt100::Parser::new(5, 20, 100);
        feed(&mut p, 1..=n);
        p
    }

    fn feed(p: &mut vt100::Parser, lines: std::ops::RangeInclusive<usize>) {
        for i in lines {
            p.process(format!("line {i:02}\r\n").as_bytes());
        }
    }

    fn top_row(p: &vt100::Parser) -> String {
        p.screen()
            .contents()
            .lines()
            .next()
            .unwrap_or("")
            .to_string()
    }

    #[test]
    fn the_wheel_stops_at_the_oldest_line_and_at_the_live_screen() {
        let mut p = fed(20);
        scroll(p.screen_mut(), 1000);
        assert_eq!(p.screen().scrollback(), 16, "no further back than held");
        assert_eq!(top_row(&p), "line 01");
        scroll(p.screen_mut(), -1000);
        assert_eq!(
            p.screen().scrollback(),
            0,
            "and no further forward than now"
        );
        scroll(p.screen_mut(), -3);
        assert_eq!(p.screen().scrollback(), 0);
    }

    /// What makes reading back usable while a command is still printing: each
    /// line pushed into history moves the offset with it, so the text in view
    /// holds still.
    #[test]
    fn output_while_scrolled_back_keeps_the_view_on_the_same_lines() {
        let mut p = fed(20);
        scroll(p.screen_mut(), 3);
        assert_eq!(top_row(&p), "line 14");

        feed(&mut p, 21..=22);
        assert_eq!(p.screen().scrollback(), 5, "the offset followed the output");
        assert_eq!(top_row(&p), "line 14", "and the view did not move");
    }

    /// The boundary the operator drew: a full-screen program is not scrolled.
    /// vt100 gives the alternate screen no history, and resets the shell's
    /// view to live on the way in.
    #[test]
    fn the_alternate_screen_has_no_history_and_does_not_scroll() {
        let mut p = fed(20);
        scroll(p.screen_mut(), 3);

        p.process(b"\x1b[?1049h");
        scroll(p.screen_mut(), 5);
        assert_eq!(p.screen().scrollback(), 0, "the wheel did nothing");

        // Leaving it, the shell's history is all still there.
        p.process(b"\x1b[?1049l");
        scroll(p.screen_mut(), 1000);
        assert_eq!(p.screen().scrollback(), 16);
    }

    /// A program that asked for the mouse owns the wheel, even where it cannot
    /// be told a position.
    #[test]
    fn a_program_that_asked_for_the_mouse_keeps_its_history_still() {
        let mut p = fed(20);
        p.process(b"\x1b[?1000h");
        scroll(p.screen_mut(), 3);
        assert_eq!(p.screen().scrollback(), 0);
    }
}
