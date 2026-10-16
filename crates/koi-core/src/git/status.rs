//! Changes in a working tree.

use std::path::Path;

use super::{Result, git};

/// How a file differs from the last commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    /// Not tracked by git yet.
    Untracked,
    /// Has merge conflicts.
    Conflicted,
}

/// A changed file, staged or not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChange {
    /// Path relative to the working tree.
    pub path: String,
    pub kind: ChangeKind,
    /// The path before a rename.
    pub original: Option<String>,
}

/// Whether the working tree at `dir` has staged, unstaged or untracked changes.
pub fn has_uncommitted_changes(dir: &Path) -> Result<bool> {
    changes(dir).map(|changes| !changes.is_empty())
}

/// Changed files in the working tree at `dir`, in path order.
///
/// Safe to run in the background: it never rewrites the index, so it does not
/// trigger watchers of the git directory.
pub fn changes(dir: &Path) -> Result<Vec<FileChange>> {
    let args = [
        "--no-optional-locks",
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
    ];
    let output = git(dir, &args)?;
    let mut changes = parse_status(&output);
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(changes)
}

/// Parses `git status --porcelain=v1 -z`: entries `XY path` separated by NUL,
/// where a rename is followed by one more entry holding the original path.
fn parse_status(output: &str) -> Vec<FileChange> {
    let mut entries = output.split('\0').filter(|entry| !entry.is_empty());
    let mut changes = Vec::new();
    while let Some(entry) = entries.next() {
        let (Some(code), Some(path)) = (entry.get(..2), entry.get(3..)) else {
            continue;
        };
        let kind = kind_of(code);
        let original = match kind {
            ChangeKind::Renamed => entries.next().map(str::to_owned),
            _ => None,
        };
        changes.push(FileChange {
            path: path.to_owned(),
            kind,
            original,
        });
    }
    changes
}

/// The change described by a two-letter status code: index state, then
/// working tree state.
fn kind_of(code: &str) -> ChangeKind {
    match code {
        "??" => ChangeKind::Untracked,
        "DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU" => ChangeKind::Conflicted,
        _ if code.starts_with('R') || code.starts_with('C') => ChangeKind::Renamed,
        _ if code.starts_with('A') => ChangeKind::Added,
        _ if code.contains('D') => ChangeKind::Deleted,
        _ => ChangeKind::Modified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::testing::init_repo;
    use crate::test_support::TempDir;

    #[test]
    fn parses_status_codes() {
        let output = " M src/main.rs\0A  new file.rs\0R  renamed.rs\0old.rs\0 D gone.rs\0?? 笔记.md\0UU both.rs\0";
        let parsed = parse_status(output);
        let summary: Vec<(&str, ChangeKind, Option<&str>)> = parsed
            .iter()
            .map(|change| {
                (
                    change.path.as_str(),
                    change.kind,
                    change.original.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("src/main.rs", ChangeKind::Modified, None),
                ("new file.rs", ChangeKind::Added, None),
                ("renamed.rs", ChangeKind::Renamed, Some("old.rs")),
                ("gone.rs", ChangeKind::Deleted, None),
                ("笔记.md", ChangeKind::Untracked, None),
                ("both.rs", ChangeKind::Conflicted, None),
            ]
        );
    }

    #[test]
    fn lists_changes_in_a_repository() {
        let temp = TempDir::new();
        let repo = temp.path();
        init_repo(repo);
        std::fs::write(repo.join("tracked.txt"), "one\n").unwrap();
        git(repo, &["add", "tracked.txt"]).unwrap();
        git(repo, &["commit", "-q", "-m", "add file"]).unwrap();
        assert!(!has_uncommitted_changes(repo).unwrap());

        std::fs::write(repo.join("tracked.txt"), "two\n").unwrap();
        std::fs::create_dir(repo.join("dir")).unwrap();
        std::fs::write(repo.join("dir/new.txt"), "new\n").unwrap();
        let found: Vec<(String, ChangeKind)> = changes(repo)
            .unwrap()
            .into_iter()
            .map(|change| (change.path, change.kind))
            .collect();
        assert_eq!(
            found,
            [
                ("dir/new.txt".to_owned(), ChangeKind::Untracked),
                ("tracked.txt".to_owned(), ChangeKind::Modified),
            ]
        );
    }
}
