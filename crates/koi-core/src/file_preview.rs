//! Decides how a file can be previewed and where a Markdown image points.

use std::path::{Path, PathBuf};

/// A rendered view a file can be shown as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preview {
    /// Shown only as a picture; there is no text to edit.
    Image,
    /// Edited as text, with an optional rendered view beside it.
    Markdown,
}

/// The preview a file supports, judged by its extension.
pub fn preview_for(path: &str) -> Option<Preview> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let (_, extension) = name.rsplit_once('.')?;
    match extension.to_ascii_lowercase().as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "svg" | "tif" | "tiff" => {
            Some(Preview::Image)
        }
        "md" | "markdown" => Some(Preview::Markdown),
        _ => None,
    }
}

/// The local file a Markdown image refers to, or `None` for remote and
/// embedded images.
///
/// Relative paths are resolved against the folder of `document`; a query or
/// fragment is ignored.
pub fn local_image(document: &Path, url: &str) -> Option<PathBuf> {
    let url = url.strip_prefix("file://").unwrap_or(url);
    if has_scheme(url) {
        return None;
    }
    let url = url.split(['?', '#']).next().unwrap_or_default();
    if url.is_empty() {
        return None;
    }
    let path = Path::new(url);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }
    Some(document.parent().unwrap_or(document).join(path))
}

/// Whether `url` starts with a scheme such as `https:` or `data:`.
fn has_scheme(url: &str) -> bool {
    url.split_once(':').is_some_and(|(scheme, _)| {
        scheme.len() > 1
            && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

#[cfg(test)]
mod tests {
    use super::{Preview, local_image, preview_for};
    use std::path::{Path, PathBuf};

    #[test]
    fn previews_follow_the_extension() {
        for (file, expected) in [
            ("assets/logo.PNG", Some(Preview::Image)),
            ("icon.svg", Some(Preview::Image)),
            ("photo.jpeg", Some(Preview::Image)),
            ("docs/README.md", Some(Preview::Markdown)),
            ("notes.markdown", Some(Preview::Markdown)),
            ("main.rs", None),
            ("Makefile", None),
            ("dir.png/file", None),
        ] {
            assert_eq!(preview_for(file), expected, "{file}");
        }
    }

    #[test]
    fn local_images_resolve_beside_the_document() {
        let document = Path::new("/repo/docs/guide.md");
        assert_eq!(
            local_image(document, "img/a.png"),
            Some(PathBuf::from("/repo/docs/img/a.png"))
        );
        assert_eq!(
            local_image(document, "../logo.svg?raw=true#top"),
            Some(PathBuf::from("/repo/docs/../logo.svg"))
        );
        assert_eq!(
            local_image(document, "/abs/b.gif"),
            Some(PathBuf::from("/abs/b.gif"))
        );
        assert_eq!(
            local_image(document, "file:///abs/c.png"),
            Some(PathBuf::from("/abs/c.png"))
        );
    }

    #[test]
    fn remote_and_embedded_images_are_not_local() {
        let document = Path::new("/repo/README.md");
        for url in [
            "https://example.com/a.png",
            "http://example.com/a.png",
            "data:image/png;base64,AAAA",
            "",
            "#anchor",
        ] {
            assert_eq!(local_image(document, url), None, "{url}");
        }
    }
}
