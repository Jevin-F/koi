//! Rendered views of open files: pictures, and Markdown in place of its source.

use std::path::PathBuf;
use std::time::SystemTime;

use gpui_kit::base::{TextView, TextViewState};
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use koi_core::file_preview::local_image;
use rust_i18n::t;

/// A picture file shown at its own size, scaled down to fit.
pub struct ImagePreview {
    path: PathBuf,
    /// Decoded frames, released when the tab closes.
    cache: Entity<RetainAllImageCache>,
    modified: Option<SystemTime>,
}

impl ImagePreview {
    pub fn new(path: PathBuf, cx: &mut App) -> Self {
        Self {
            path,
            cache: RetainAllImageCache::new(cx),
            modified: None,
        }
    }

    /// Decodes the picture again only when the file changed on disk.
    pub fn refresh(&mut self, modified: Option<SystemTime>, window: &mut Window, cx: &mut App) {
        if self.modified != modified {
            self.modified = modified;
            self.cache.update(cx, |cache, cx| cache.clear(window, cx));
        }
    }

    pub fn render(&self, cx: &App) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        image_cache(self.cache.clone())
            .size_full()
            .p_4()
            .child(
                img(self.path.clone())
                    .size_full()
                    .object_fit(ObjectFit::ScaleDown)
                    .with_fallback(move || {
                        div()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_sm()
                            .text_color(muted)
                            .child(t!("files.image_unreadable"))
                            .into_any_element()
                    }),
            )
            .into_any_element()
    }
}

/// The rendered form of a Markdown file, shown instead of its editor.
pub struct MarkdownPreview {
    /// Absolute path of the document, used to find its relative images.
    document: PathBuf,
    text: Entity<TextViewState>,
    /// Pictures in the document, released when the preview closes.
    images: Entity<RetainAllImageCache>,
}

impl MarkdownPreview {
    pub fn new(document: PathBuf, source: &str, cx: &mut App) -> Self {
        Self {
            document,
            text: cx.new(|cx| TextViewState::markdown(source, cx)),
            images: RetainAllImageCache::new(cx),
        }
    }

    /// Shows `source`, such as the file's new text after a change on disk.
    pub fn set_source(&self, source: &str, cx: &mut App) {
        self.text.update(cx, |text, cx| text.set_text(source, cx));
    }

    pub fn render(&self) -> AnyElement {
        let document = self.document.clone();
        image_cache(self.images.clone())
            .size_full()
            .child(
                TextView::new(&self.text)
                    .scrollable(true)
                    .selectable(true)
                    .image_source(move |url| match local_image(&document, url) {
                        Some(path) => path.into(),
                        None => url.clone().into(),
                    })
                    .size_full()
                    .px_4(),
            )
            .into_any_element()
    }
}
