//! Git change marks for the file tree: the change of each file, and the most
//! notable change inside each folder.

use std::collections::HashMap;

use crate::git::{ChangeKind, FileChange};

/// Changes looked up by tree path.
#[derive(Default)]
pub struct ChangeMarks {
    files: HashMap<String, ChangeKind>,
    folders: HashMap<String, ChangeKind>,
}

impl ChangeMarks {
    /// Marks for the changes of a worktree.
    pub fn new(changes: &[FileChange]) -> Self {
        let mut marks = Self::default();
        for change in changes {
            marks.files.insert(change.path.clone(), change.kind);
            let mut end = 0;
            while let Some(offset) = change.path[end..].find('/') {
                end += offset;
                marks
                    .folders
                    .entry(change.path[..end].to_owned())
                    .and_modify(|kind| {
                        if rank(change.kind) > rank(*kind) {
                            *kind = change.kind;
                        }
                    })
                    .or_insert(change.kind);
                end += 1;
            }
        }
        marks
    }

    /// The change of the file at `path`.
    pub fn file(&self, path: &str) -> Option<ChangeKind> {
        self.files.get(path).copied()
    }

    /// The most notable change among the files below the folder at `path`.
    pub fn folder(&self, path: &str) -> Option<ChangeKind> {
        self.folders.get(path).copied()
    }
}

/// How much a change stands out when a folder can show only one.
fn rank(kind: ChangeKind) -> u8 {
    match kind {
        ChangeKind::Untracked => 0,
        ChangeKind::Added => 1,
        ChangeKind::Renamed => 2,
        ChangeKind::Modified => 3,
        ChangeKind::Deleted => 4,
        ChangeKind::Conflicted => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::ChangeMarks;
    use crate::git::{ChangeKind, FileChange};

    fn change(path: &str, kind: ChangeKind) -> FileChange {
        FileChange {
            path: path.to_owned(),
            kind,
            original: None,
        }
    }

    #[test]
    fn files_keep_their_change_and_folders_the_most_notable_one() {
        let marks = ChangeMarks::new(&[
            change("README.md", ChangeKind::Modified),
            change("src/new.rs", ChangeKind::Untracked),
            change("src/ui/view.rs", ChangeKind::Modified),
            change("src/ui/old.rs", ChangeKind::Deleted),
            change("docs/a.md", ChangeKind::Added),
        ]);
        assert_eq!(marks.file("README.md"), Some(ChangeKind::Modified));
        assert_eq!(marks.file("src/new.rs"), Some(ChangeKind::Untracked));
        assert_eq!(marks.file("src/main.rs"), None);
        assert_eq!(marks.folder("src"), Some(ChangeKind::Deleted));
        assert_eq!(marks.folder("src/ui"), Some(ChangeKind::Deleted));
        assert_eq!(marks.folder("docs"), Some(ChangeKind::Added));
        assert_eq!(marks.folder("README.md"), None);
        assert_eq!(marks.folder("lib"), None);
    }
}
