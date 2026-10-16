//! Keeps the parts of the app that macOS draws, such as window title bars and
//! file dialogs, in the appearance chosen in Koi's settings. Without this
//! they follow the system even when Koi is set to light or dark.

use koi_core::settings::Appearance;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSAppearance, NSApplication};
use objc2_foundation::NSString;

/// Name of the macOS appearance matching `appearance`; `None` follows the
/// system. AppKit's appearance name constants equal their own names.
fn appearance_name(appearance: Appearance) -> Option<&'static str> {
    match appearance {
        Appearance::Dark => Some("NSAppearanceNameDarkAqua"),
        Appearance::Light => Some("NSAppearanceNameAqua"),
        Appearance::System => None,
    }
}

/// Applies `appearance` to every window of the app. Does nothing off the
/// main thread, where AppKit is unavailable, as in tests.
pub fn apply(appearance: Appearance) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let native = appearance_name(appearance)
        .and_then(|name| NSAppearance::appearanceNamed(&NSString::from_str(name)));
    NSApplication::sharedApplication(mtm).setAppearance(native.as_deref());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_leaves_the_choice_to_macos() {
        assert_eq!(appearance_name(Appearance::System), None);
        assert_eq!(
            appearance_name(Appearance::Dark),
            Some("NSAppearanceNameDarkAqua")
        );
        assert_eq!(
            appearance_name(Appearance::Light),
            Some("NSAppearanceNameAqua")
        );
    }
}
