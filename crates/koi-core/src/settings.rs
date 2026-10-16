//! Preferences set in the settings window, saved as `settings.json`.

use std::collections::BTreeMap;
use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};

/// Light or dark interface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    Dark,
    Light,
    /// Follows the macOS setting.
    System,
}

/// Language of the interface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "zh-CN")]
    Chinese,
    #[serde(rename = "en")]
    English,
}

impl Language {
    /// The locale name used by the translations.
    pub fn locale(self) -> &'static str {
        match self {
            Self::Chinese => "zh-CN",
            Self::English => "en",
        }
    }
}

/// Terminal font sizes offered, in points.
pub const FONT_SIZES: RangeInclusive<f32> = 9.0..=28.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub appearance: Appearance,
    pub language: Language,
    /// Font of terminals and code.
    pub font_family: String,
    font_size: f32,
    /// Shortcuts replacing the defaults, keyed by action name such as
    /// `workspace::QuickOpen`.
    pub shortcuts: BTreeMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            appearance: Appearance::default(),
            language: Language::default(),
            font_family: "Menlo".to_owned(),
            font_size: 13.,
            shortcuts: BTreeMap::new(),
        }
    }
}

impl Settings {
    /// The font size, kept within [`FONT_SIZES`] even if the file says otherwise.
    pub fn font_size(&self) -> f32 {
        self.font_size.clamp(*FONT_SIZES.start(), *FONT_SIZES.end())
    }

    pub fn set_font_size(&mut self, size: f32) {
        self.font_size = size.clamp(*FONT_SIZES.start(), *FONT_SIZES.end());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_take_defaults() {
        let settings: Settings = serde_json::from_str(r#"{"language": "en"}"#).unwrap();
        assert_eq!(settings.language, Language::English);
        assert_eq!(settings.appearance, Appearance::Dark);
        assert_eq!(settings.font_family, "Menlo");
    }

    #[test]
    fn font_size_stays_in_range() {
        let mut settings: Settings = serde_json::from_str(r#"{"font_size": 300}"#).unwrap();
        assert_eq!(settings.font_size(), *FONT_SIZES.end());
        settings.set_font_size(1.);
        assert_eq!(settings.font_size(), *FONT_SIZES.start());
    }

    #[test]
    fn round_trips_through_json() {
        let mut settings = Settings {
            appearance: Appearance::System,
            ..Settings::default()
        };
        settings
            .shortcuts
            .insert("workspace::QuickOpen".into(), "cmd-k".into());
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains(r#""appearance":"system""#));
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), settings);
    }
}
