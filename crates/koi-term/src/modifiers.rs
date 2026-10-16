//! Modifier keys forwarded to terminal applications.

/// Modifier keys that reach the terminal. Command is reserved for the app.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub control: bool,
}

impl Modifiers {
    pub(crate) fn any(self) -> bool {
        self.shift || self.alt || self.control
    }

    /// The xterm modifier parameter: 1 plus a bit per held modifier.
    pub(crate) fn param(self) -> u8 {
        1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.control)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xterm_parameter() {
        assert_eq!(Modifiers::default().param(), 1);
        let all = Modifiers {
            shift: true,
            alt: true,
            control: true,
        };
        assert_eq!(all.param(), 8);
    }
}
