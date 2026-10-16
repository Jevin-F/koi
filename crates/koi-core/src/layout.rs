//! Sidebar sizing independent of the window's flexible content area.

use serde::{Deserialize, Serialize};

const MIN_SIDEBAR: u16 = 280;
const MAX_SIDEBAR: u16 = 600;
const MIN_CONTENT: u16 = 300;

/// The user's preferred sidebar width, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SidebarWidth(u16);

impl Default for SidebarWidth {
    fn default() -> Self {
        Self(360)
    }
}

impl SidebarWidth {
    /// Records a manual resize within the supported width range.
    pub fn resize(&mut self, width: u16) {
        self.0 = width.clamp(MIN_SIDEBAR, MAX_SIDEBAR);
    }

    /// Fits the sidebar without changing the preference when the window shrinks.
    pub fn in_window(self, width: u16) -> u16 {
        self.0
            .clamp(MIN_SIDEBAR, MAX_SIDEBAR)
            .min(width.saturating_sub(MIN_CONTENT))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_resizes_preserve_the_chosen_width() {
        let mut sidebar = SidebarWidth::default();
        sidebar.resize(420);
        assert_eq!(sidebar.in_window(1200), 420);
        assert_eq!(sidebar.in_window(1600), 420);
        assert_eq!(sidebar.in_window(640), 340);
        assert_eq!(sidebar.in_window(1200), 420);
    }

    #[test]
    fn bounds_and_saved_preferences_are_respected() {
        let mut sidebar = SidebarWidth::default();
        sidebar.resize(0);
        assert_eq!(sidebar.in_window(1200), MIN_SIDEBAR);
        sidebar.resize(u16::MAX);
        assert_eq!(sidebar.in_window(1200), MAX_SIDEBAR);
        assert_eq!(sidebar.in_window(200), 0);
        let loaded: SidebarWidth = serde_json::from_str("65535").unwrap();
        assert_eq!(loaded.in_window(1200), MAX_SIDEBAR);
        assert_eq!(
            serde_json::from_str::<SidebarWidth>(&serde_json::to_string(&sidebar).unwrap())
                .unwrap(),
            sidebar
        );
    }
}
