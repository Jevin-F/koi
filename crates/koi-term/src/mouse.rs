//! Encodes mouse events for applications that enable mouse reporting.

use alacritty_terminal::term::TermMode;

use crate::Modifiers;

/// A mouse button, including the two scroll-wheel directions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
}

impl MouseButton {
    fn code(self) -> u8 {
        match self {
            Self::Left => 0,
            Self::Middle => 1,
            Self::Right => 2,
            Self::WheelUp => 64,
            Self::WheelDown => 65,
        }
    }
}

/// What happened to the mouse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseEvent {
    Press(MouseButton),
    Release(MouseButton),
    /// Movement while a button is held.
    Drag(MouseButton),
    /// Movement with no button held.
    Move,
}

/// Returns `true` if the application asked to receive this kind of event.
pub fn reports(event: MouseEvent, mode: TermMode) -> bool {
    match event {
        MouseEvent::Press(_) | MouseEvent::Release(_) => mode.intersects(TermMode::MOUSE_MODE),
        MouseEvent::Drag(_) => mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION),
        MouseEvent::Move => mode.contains(TermMode::MOUSE_MOTION),
    }
}

/// Encodes `event` at a zero-based viewport cell. Returns `None` when the
/// application did not ask for it or the legacy encoding cannot express it.
pub fn encode_mouse(
    event: MouseEvent,
    mods: Modifiers,
    column: usize,
    line: usize,
    mode: TermMode,
) -> Option<Vec<u8>> {
    if !reports(event, mode) {
        return None;
    }
    let (button, released) = match event {
        MouseEvent::Press(button) => (button.code(), false),
        MouseEvent::Release(MouseButton::WheelUp | MouseButton::WheelDown) => return None,
        MouseEvent::Release(button) => (button.code(), true),
        MouseEvent::Drag(button) => (button.code() + 32, false),
        MouseEvent::Move => (3 + 32, false),
    };
    let code =
        button + 4 * u8::from(mods.shift) + 8 * u8::from(mods.alt) + 16 * u8::from(mods.control);
    let (x, y) = (column + 1, line + 1);

    if mode.contains(TermMode::SGR_MOUSE) {
        let suffix = if released { 'm' } else { 'M' };
        return Some(format!("\x1b[<{code};{x};{y}{suffix}").into_bytes());
    }
    // The legacy encoding has no button for releases and sends each value in one byte.
    let code = if released { 3 + (code & !3) } else { code };
    let byte = |value: usize| u8::try_from(value + 32).ok();
    Some(vec![
        0x1b,
        b'[',
        b'M',
        byte(usize::from(code))?,
        byte(x)?,
        byte(y)?,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLICKS: TermMode = TermMode::MOUSE_REPORT_CLICK;

    #[test]
    fn nothing_is_reported_without_a_mouse_mode() {
        let press = MouseEvent::Press(MouseButton::Left);
        assert_eq!(
            encode_mouse(press, Modifiers::default(), 0, 0, TermMode::empty()),
            None
        );
    }

    #[test]
    fn sgr_press_and_release() {
        let mode = CLICKS | TermMode::SGR_MOUSE;
        let mods = Modifiers::default();
        let press = encode_mouse(MouseEvent::Press(MouseButton::Left), mods, 4, 9, mode);
        assert_eq!(press.unwrap(), b"\x1b[<0;5;10M");
        let release = encode_mouse(MouseEvent::Release(MouseButton::Left), mods, 4, 9, mode);
        assert_eq!(release.unwrap(), b"\x1b[<0;5;10m");
    }

    #[test]
    fn sgr_wheel_with_modifier() {
        let mode = CLICKS | TermMode::SGR_MOUSE;
        let mods = Modifiers {
            control: true,
            ..Modifiers::default()
        };
        let wheel = encode_mouse(MouseEvent::Press(MouseButton::WheelDown), mods, 0, 0, mode);
        assert_eq!(wheel.unwrap(), b"\x1b[<81;1;1M");
    }

    #[test]
    fn legacy_encoding() {
        let mods = Modifiers::default();
        let press = encode_mouse(MouseEvent::Press(MouseButton::Right), mods, 0, 1, CLICKS);
        assert_eq!(press.unwrap(), [0x1b, b'[', b'M', 34, 33, 34]);
        let release = encode_mouse(MouseEvent::Release(MouseButton::Right), mods, 0, 1, CLICKS);
        assert_eq!(release.unwrap(), [0x1b, b'[', b'M', 35, 33, 34]);
        assert_eq!(
            encode_mouse(MouseEvent::Press(MouseButton::Left), mods, 300, 0, CLICKS),
            None
        );
    }

    #[test]
    fn motion_requires_motion_modes() {
        assert!(!reports(MouseEvent::Drag(MouseButton::Left), CLICKS));
        assert!(reports(
            MouseEvent::Drag(MouseButton::Left),
            TermMode::MOUSE_DRAG
        ));
        assert!(!reports(MouseEvent::Move, TermMode::MOUSE_DRAG));
        assert!(reports(MouseEvent::Move, TermMode::MOUSE_MOTION));
    }
}
