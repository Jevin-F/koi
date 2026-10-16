//! Keyboard and input-method handling.
//!
//! Plain printable keys are left to the platform text input path, so input
//! methods and dead keys work; everything else is encoded here.

use std::ops::Range;

use gpui_kit::*;
use koi_term::encode_key;
use koi_term::state::grid::Dimensions;

use super::{TerminalEvent, TerminalView, submitted_line, term_modifiers};

impl TerminalView {
    pub(super) fn on_key_down(
        &mut self,
        event: &KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        let held = keystroke.modifiers;
        // Command shortcuts belong to the app; composition belongs to the input method.
        if held.platform || self.marked_text.is_some() {
            return;
        }
        let produces_text = keystroke
            .key_char
            .as_ref()
            .is_some_and(|text| !text.chars().any(char::is_control));
        if produces_text && !held.control && !held.alt {
            return;
        }
        if let Some(bytes) = encode_key(&keystroke.key, term_modifiers(&held), self.mode()) {
            let submitted = (keystroke.key == "enter" && held == Modifiers::none())
                .then(|| submitted_line(&self.session.term()));
            self.send_input(&bytes, cx);
            if let Some(prompt) = submitted {
                self.session.submit();
                cx.emit(TerminalEvent::Submitted(prompt));
            }
            cx.stop_propagation();
        }
    }

    /// Bounds of the cell holding the terminal cursor, clamped to the visible
    /// screen. Reported even while the cursor is hidden: full-screen apps hide
    /// it but leave it where text is entered.
    fn cursor_cell(&self) -> Option<Bounds<Pixels>> {
        let geometry = self.geometry?;
        let term = self.session.term();
        let grid = term.grid();
        let last_line = grid.screen_lines().saturating_sub(1) as i32;
        let line = (grid.cursor.point.line.0 + grid.display_offset() as i32).clamp(0, last_line);
        Some(geometry.cells(line as usize, grid.cursor.point.column.0, 1))
    }
}

impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        _: Range<usize>,
        _: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let end = self
            .marked_text
            .as_ref()
            .map_or(0, |text| text.encode_utf16().count());
        Some(UTF16Selection {
            range: end..end,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_text
            .as_ref()
            .map(|text| 0..text.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked_text = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked_text = None;
        self.send_input(text.as_bytes(), cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked_text = (!text.is_empty()).then(|| text.to_owned());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.cursor_cell()
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}
