//! Open editor tabs and their unsaved state.

use std::collections::HashSet;

/// File tabs in display order, with one active file.
#[derive(Default)]
pub struct FileTabs {
    paths: Vec<String>,
    active: Option<String>,
    dirty: HashSet<String>,
}

impl FileTabs {
    /// Opens or selects a file, returning whether it is new.
    pub fn open(&mut self, path: String) -> bool {
        let is_new = !self.paths.contains(&path);
        if is_new {
            self.paths.push(path.clone());
        }
        self.active = Some(path);
        is_new
    }

    /// Closes a file and selects its next neighbor when needed.
    pub fn close(&mut self, path: &str) {
        let Some(index) = self.paths.iter().position(|candidate| candidate == path) else {
            return;
        };
        self.paths.remove(index);
        self.dirty.remove(path);
        if self.active.as_deref() == Some(path) {
            self.active = self.paths.get(index).or_else(|| self.paths.last()).cloned();
        }
    }

    /// Ordered file paths.
    pub fn paths(&self) -> &[String] {
        &self.paths
    }

    /// Currently shown file, if any.
    pub fn active(&self) -> Option<&str> {
        self.active.as_deref()
    }

    /// Whether a file has unsaved edits.
    pub fn is_dirty(&self, path: &str) -> bool {
        self.dirty.contains(path)
    }

    /// Whether any file has unsaved edits.
    pub fn has_unsaved(&self) -> bool {
        !self.dirty.is_empty()
    }

    /// Updates the unsaved state of an open file.
    pub fn set_dirty(&mut self, path: &str, dirty: bool) {
        if !self.paths.iter().any(|candidate| candidate == path) {
            return;
        }
        if dirty {
            self.dirty.insert(path.to_owned());
        } else {
            self.dirty.remove(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FileTabs;

    #[test]
    fn opening_an_existing_file_keeps_one_tab_and_its_edits() {
        let mut tabs = FileTabs::default();
        assert!(tabs.open("a.rs".into()));
        tabs.set_dirty("a.rs", true);
        assert!(tabs.open("b.rs".into()));
        assert!(!tabs.open("a.rs".into()));
        assert_eq!(tabs.paths(), ["a.rs", "b.rs"]);
        assert_eq!(tabs.active(), Some("a.rs"));
        assert!(tabs.is_dirty("a.rs"));
    }

    #[test]
    fn closing_selects_a_neighbor_and_clears_only_that_files_edits() {
        let mut tabs = FileTabs::default();
        for path in ["a", "b", "c"] {
            tabs.open(path.into());
        }
        tabs.set_dirty("a", true);
        tabs.set_dirty("b", true);
        tabs.open("b".into());
        tabs.close("b");
        assert_eq!(tabs.active(), Some("c"));
        assert!(tabs.is_dirty("a"));
        tabs.close("c");
        assert_eq!(tabs.active(), Some("a"));
        tabs.close("a");
        assert_eq!(tabs.active(), None);
        assert!(!tabs.has_unsaved());
    }
}
