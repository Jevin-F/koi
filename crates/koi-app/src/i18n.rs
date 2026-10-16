//! Interface language. Texts live in `locales/app.toml`; gpui-kit components
//! share the same locale setting.

use koi_core::settings::Language;

/// Switches the interface, including gpui-kit's components, to `language`.
pub fn set_language(language: Language) {
    rust_i18n::set_locale(language.locale());
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    const LOCALES: [&str; 2] = ["en", "zh-CN"];

    /// Every `key -> {locale -> text}` entry of the locale file.
    fn entries() -> Vec<(String, toml::Table)> {
        fn walk(prefix: &str, table: &toml::Table, out: &mut Vec<(String, toml::Table)>) {
            for (name, value) in table {
                let Some(child) = value.as_table() else {
                    continue;
                };
                let key = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}.{name}")
                };
                if child.values().all(toml::Value::is_str) {
                    out.push((key, child.clone()));
                } else {
                    walk(&key, child, out);
                }
            }
        }
        let text = include_str!("../locales/app.toml");
        let mut out = Vec::new();
        walk(
            "",
            &toml::from_str(text).expect("locale file parses"),
            &mut out,
        );
        out
    }

    fn placeholders(text: &str) -> BTreeSet<&str> {
        text.split("%{")
            .skip(1)
            .filter_map(|rest| rest.split_once('}').map(|(name, _)| name))
            .collect()
    }

    #[test]
    fn every_text_is_translated_consistently() {
        for (key, texts) in entries() {
            let locales: Vec<&str> = texts.keys().map(String::as_str).collect();
            assert_eq!(locales, LOCALES, "{key}");
            let english = placeholders(texts["en"].as_str().unwrap());
            let chinese = placeholders(texts["zh-CN"].as_str().unwrap());
            assert_eq!(english, chinese, "placeholders differ in {key}");
        }
    }

    #[test]
    fn every_key_in_the_code_exists() {
        fn sources(dir: &Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    sources(&path, out);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    out.push(std::fs::read_to_string(path).unwrap());
                }
            }
        }
        let mut files = Vec::new();
        sources(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut files,
        );
        let known: BTreeSet<String> = entries().into_iter().map(|(key, _)| key).collect();
        let marker = concat!("t!", "(\"");
        let used = files.iter().flat_map(|file| {
            file.match_indices(marker)
                // Skip macros merely ending in `t!`, such as `format!`.
                .filter(|(at, _)| !file[..*at].ends_with(|c: char| c.is_alphanumeric() || c == '_'))
                .filter_map(|(at, _)| file[at + marker.len()..].split_once('"'))
                .map(|(key, _)| key.to_owned())
        });
        for key in used {
            assert!(known.contains(&key), "missing translation for {key}");
        }
    }
}
