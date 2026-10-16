//! Mouse input: text selection, scrollback, links and reporting to
//! applications.
//!
//! When the application enables mouse reporting, events go to it; holding
//! shift selects text instead. Command-click opens a web link.

use gpui_kit::*;
use koi_term::state::grid::{Dimensions, Scroll};
use koi_term::state::index::{Column, Line, Side};
use koi_term::state::selection::{Selection, SelectionType};
use koi_term::state::term::TermMode;
use koi_term::{
    MouseButton as TermButton, MouseEvent as TermMouse, encode_key, encode_mouse, link_at,
};

use super::{TerminalView, term_modifiers};

/// A viewport cell under the pointer, and which half of it.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct CellHit {
    line: usize,
    column: usize,
    side: Side,
}

fn term_button(button: MouseButton) -> Option<TermButton> {
    match button {
        MouseButton::Left => Some(TermButton::Left),
        MouseButton::Middle => Some(TermButton::Middle),
        MouseButton::Right => Some(TermButton::Right),
        MouseButton::Navigate(_) => None,
    }
}

impl TerminalView {
    fn hit(&self, position: Point<Pixels>) -> Option<CellHit> {
        let geometry = self.geometry?;
        let term = self.session.term();
        let offset = position - geometry.origin;
        let x = (offset.x / geometry.cell.width).max(0.);
        let y = (offset.y / geometry.cell.height).max(0.);
        Some(CellHit {
            line: (y as usize).min(term.screen_lines() - 1),
            column: (x as usize).min(term.columns() - 1),
            side: if x.fract() < 0.5 {
                Side::Left
            } else {
                Side::Right
            },
        })
    }

    fn reports_mouse(&self, modifiers: &Modifiers) -> bool {
        !modifiers.shift && self.mode().intersects(TermMode::MOUSE_MODE)
    }

    fn report(&self, event: TermMouse, modifiers: &Modifiers, hit: CellHit) {
        let bytes = encode_mouse(
            event,
            term_modifiers(modifiers),
            hit.column,
            hit.line,
            self.mode(),
        );
        if let Some(bytes) = bytes {
            self.session.write(&bytes);
        }
    }

    fn grid_point(&self, hit: CellHit) -> koi_term::state::index::Point {
        let offset = self.session.term().grid().display_offset() as i32;
        koi_term::state::index::Point::new(Line(hit.line as i32 - offset), Column(hit.column))
    }

    pub(super) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        let (Some(hit), Some(button)) = (self.hit(event.position), term_button(event.button))
        else {
            return;
        };
        if event.modifiers.platform && button == TermButton::Left {
            let point = self.grid_point(hit);
            if let Some(url) = link_at(&self.session.term(), point) {
                cx.open_url(&url);
            }
            return;
        }
        if self.reports_mouse(&event.modifiers) {
            self.last_reported = Some(hit);
            self.report(TermMouse::Press(button), &event.modifiers, hit);
            return;
        }
        if button != TermButton::Left {
            return;
        }
        let kind = match event.click_count {
            1 => SelectionType::Simple,
            2 => SelectionType::Semantic,
            _ => SelectionType::Lines,
        };
        let point = self.grid_point(hit);
        self.session.term().selection = Some(Selection::new(kind, point, hit.side));
        self.selecting = true;
        cx.notify();
    }

    pub(super) fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(hit) = self.hit(event.position) else {
            return;
        };
        if self.selecting {
            let point = self.grid_point(hit);
            if let Some(selection) = &mut self.session.term().selection {
                selection.update(point, hit.side);
            }
            cx.notify();
            return;
        }
        if !self.reports_mouse(&event.modifiers) {
            return;
        }
        // Report movement once per cell, not once per pixel.
        let moved = self
            .last_reported
            .is_none_or(|last| (last.line, last.column) != (hit.line, hit.column));
        if moved {
            self.last_reported = Some(hit);
            let motion = match event.pressed_button.and_then(term_button) {
                Some(button) => TermMouse::Drag(button),
                None => TermMouse::Move,
            };
            self.report(motion, &event.modifiers, hit);
        }
    }

    pub(super) fn on_mouse_up(
        &mut self,
        event: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selecting {
            self.selecting = false;
            let mut term = self.session.term();
            if term.selection.as_ref().is_some_and(Selection::is_empty) {
                term.selection = None;
            }
            cx.notify();
            return;
        }
        if self.reports_mouse(&event.modifiers)
            && let (Some(hit), Some(button)) = (self.hit(event.position), term_button(event.button))
        {
            self.report(TermMouse::Release(button), &event.modifiers, hit);
        }
    }

    pub(super) fn on_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(geometry) = self.geometry else {
            return;
        };
        let delta = event.delta.pixel_delta(geometry.cell.height).y;
        self.scroll_remainder += delta / geometry.cell.height;
        let lines = self.scroll_remainder.trunc();
        self.scroll_remainder -= lines;
        if lines == 0. {
            return;
        }
        let (count, up) = (lines.abs() as usize, lines > 0.);
        let mode = self.mode();

        if self.reports_mouse(&event.modifiers) {
            let Some(hit) = self.hit(event.position) else {
                return;
            };
            let wheel = if up {
                TermButton::WheelUp
            } else {
                TermButton::WheelDown
            };
            for _ in 0..count {
                self.report(TermMouse::Press(wheel), &event.modifiers, hit);
            }
        } else if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
            // Full-screen apps without mouse support scroll with the arrow keys.
            let key = if up { "up" } else { "down" };
            if let Some(bytes) = encode_key(key, koi_term::Modifiers::default(), mode) {
                self.session.write(&bytes.repeat(count));
            }
        } else {
            self.session
                .term()
                .scroll_display(Scroll::Delta(lines as i32));
            cx.notify();
        }
    }
}
