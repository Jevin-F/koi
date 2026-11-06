//! A directory tree built from a flat list of file paths, shown as rows
//! with folders expanded or collapsed.

use std::collections::{BTreeMap, HashSet};

/// A line of the tree as displayed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRow {
    /// Nesting level, zero at the top.
    pub depth: usize,
    pub name: String,
    /// Path from the root, with `/` separators.
    pub path: String,
    pub is_dir: bool,
    pub expanded: bool,
    /// Whether git ignores the entry.
    pub ignored: bool,
}

/// A file or folder read from disk inside an ignored folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderEntry {
    pub name: String,
    pub is_dir: bool,
}

#[derive(Default)]
struct Dir {
    dirs: BTreeMap<String, Dir>,
    files: Vec<File>,
    ignored: bool,
    /// Contents are read from disk only once the folder is expanded.
    unread: bool,
}

struct File {
    name: String,
    ignored: bool,
}

/// Folders and files of a worktree.
#[derive(Default)]
pub struct FileTree {
    root: Dir,
}

impl FileTree {
    /// Builds the tree from paths relative to the root, such as `src/main.rs`.
    pub fn new<'a>(paths: impl IntoIterator<Item = &'a str>) -> Self {
        let mut tree = Self::default();
        for path in paths {
            tree.add_file(path, false);
        }
        tree
    }

    /// Adds ignored entries: files, and folders ending in `/` whose contents
    /// are left unread until [`Self::fill`].
    pub fn add_ignored<'a>(&mut self, entries: impl IntoIterator<Item = &'a str>) {
        for entry in entries {
            match entry.strip_suffix('/') {
                Some(folder) => {
                    let dir = self.root.dir_mut(folder);
                    dir.ignored = true;
                    dir.unread = true;
                }
                None => self.add_file(entry, true),
            }
        }
    }

    fn add_file(&mut self, path: &str, ignored: bool) {
        let (folder, name) = path.rsplit_once('/').unwrap_or(("", path));
        if name.is_empty() {
            return;
        }
        self.root.dir_mut(folder).files.push(File {
            name: name.to_owned(),
            ignored,
        });
    }

    /// Folders in `expanded` that are shown but whose contents are unread.
    pub fn unread(&self, expanded: &HashSet<String>) -> Vec<String> {
        let mut unread = Vec::new();
        collect_unread(&self.root, "", expanded, &mut unread);
        unread
    }

    /// Puts the contents read from disk into an unread folder. Everything
    /// inside an ignored folder is ignored too.
    pub fn fill(&mut self, path: &str, entries: Vec<FolderEntry>) {
        let Some(dir) = self.root.find_mut(path).filter(|dir| dir.unread) else {
            return;
        };
        dir.unread = false;
        let ignored = dir.ignored;
        for entry in entries {
            if entry.is_dir {
                let child = dir.dirs.entry(entry.name).or_default();
                child.ignored = ignored;
                child.unread = true;
            } else {
                dir.files.push(File {
                    name: entry.name,
                    ignored,
                });
            }
        }
    }

    /// Visible rows: folders before files, each sorted ignoring case, with
    /// the contents of folders in `expanded` (by path) shown below them.
    pub fn rows(&self, expanded: &HashSet<String>) -> Vec<TreeRow> {
        let mut rows = Vec::new();
        push_rows(&self.root, "", 0, expanded, &mut rows);
        rows
    }
}

impl Dir {
    /// The folder at `path`, created with its parents when missing.
    fn dir_mut(&mut self, path: &str) -> &mut Dir {
        path.split('/')
            .filter(|part| !part.is_empty())
            .fold(self, |dir, part| {
                dir.dirs.entry(part.to_owned()).or_default()
            })
    }

    fn find_mut(&mut self, path: &str) -> Option<&mut Dir> {
        path.split('/')
            .try_fold(self, |dir, part| dir.dirs.get_mut(part))
    }
}

fn collect_unread(dir: &Dir, prefix: &str, expanded: &HashSet<String>, unread: &mut Vec<String>) {
    for (name, child) in &dir.dirs {
        let path = format!("{prefix}{name}");
        if !expanded.contains(&path) {
            continue;
        }
        if child.unread {
            unread.push(path);
        } else {
            collect_unread(child, &format!("{path}/"), expanded, unread);
        }
    }
}

fn push_rows(
    dir: &Dir,
    prefix: &str,
    depth: usize,
    expanded: &HashSet<String>,
    rows: &mut Vec<TreeRow>,
) {
    let mut dirs: Vec<(&String, &Dir)> = dir.dirs.iter().collect();
    dirs.sort_by_key(|(name, _)| name.to_lowercase());
    for (name, child) in dirs {
        let path = format!("{prefix}{name}");
        let is_expanded = expanded.contains(&path);
        rows.push(TreeRow {
            depth,
            name: name.clone(),
            path: path.clone(),
            is_dir: true,
            expanded: is_expanded,
            ignored: child.ignored,
        });
        if is_expanded {
            push_rows(child, &format!("{path}/"), depth + 1, expanded, rows);
        }
    }
    let mut files: Vec<&File> = dir.files.iter().collect();
    files.sort_by_key(|file| file.name.to_lowercase());
    rows.extend(files.into_iter().map(|file| TreeRow {
        depth,
        name: file.name.clone(),
        path: format!("{prefix}{}", file.name),
        is_dir: false,
        expanded: false,
        ignored: file.ignored,
    }));
}

/// The folders containing `path`, outermost first: `a/b/c.rs` gives `a`
/// and `a/b`. Expanding them reveals the file.
pub fn ancestors(path: &str) -> Vec<String> {
    let mut folders = Vec::new();
    let mut end = 0;
    while let Some(offset) = path[end..].find('/') {
        end += offset;
        folders.push(path[..end].to_owned());
        end += 1;
    }
    folders
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(rows: &[TreeRow]) -> Vec<String> {
        rows.iter()
            .map(|row| {
                format!(
                    "{}{}{}",
                    "  ".repeat(row.depth),
                    row.name,
                    if row.is_dir { "/" } else { "" }
                )
            })
            .collect()
    }

    const PATHS: [&str; 5] = [
        "src/main.rs",
        "README.md",
        "src/ui/view.rs",
        "Cargo.toml",
        "src/App.rs",
    ];

    #[test]
    fn collapsed_tree_shows_the_top_level() {
        let tree = FileTree::new(PATHS);
        assert_eq!(
            names(&tree.rows(&HashSet::new())),
            ["src/", "Cargo.toml", "README.md"]
        );
    }

    #[test]
    fn expanded_folders_show_their_contents() {
        let tree = FileTree::new(PATHS);
        let expanded: HashSet<String> = ["src".to_owned(), "src/ui".to_owned()].into();
        assert_eq!(
            names(&tree.rows(&expanded)),
            [
                "src/",
                "  ui/",
                "    view.rs",
                "  App.rs",
                "  main.rs",
                "Cargo.toml",
                "README.md"
            ]
        );
        let view = tree
            .rows(&expanded)
            .into_iter()
            .find(|row| row.name == "view.rs")
            .unwrap();
        assert_eq!(view.path, "src/ui/view.rs");
    }

    #[test]
    fn ignored_folders_are_read_only_once_expanded() {
        let mut tree = FileTree::new(["src/main.rs"]);
        tree.add_ignored([".env", "node_modules/", "src/run.log"]);
        let mut expanded = HashSet::new();
        assert!(tree.unread(&expanded).is_empty());
        assert_eq!(
            names(&tree.rows(&expanded)),
            ["node_modules/", "src/", ".env"]
        );

        expanded.insert("node_modules".to_owned());
        assert_eq!(tree.unread(&expanded), ["node_modules"]);
        tree.fill(
            "node_modules",
            vec![
                FolderEntry {
                    name: "react".into(),
                    is_dir: true,
                },
                FolderEntry {
                    name: ".bin-stamp".into(),
                    is_dir: false,
                },
            ],
        );
        assert!(tree.unread(&expanded).is_empty());
        expanded.insert("node_modules/react".to_owned());
        assert_eq!(tree.unread(&expanded), ["node_modules/react"]);
        expanded.insert("src".to_owned());

        let rows = tree.rows(&expanded);
        assert_eq!(
            names(&rows),
            [
                "node_modules/",
                "  react/",
                "  .bin-stamp",
                "src/",
                "  main.rs",
                "  run.log",
                ".env"
            ]
        );
        let ignored: Vec<&str> = rows
            .iter()
            .filter(|row| row.ignored)
            .map(|row| row.name.as_str())
            .collect();
        assert_eq!(
            ignored,
            ["node_modules", "react", ".bin-stamp", "run.log", ".env"]
        );
    }

    #[test]
    fn filling_twice_or_a_missing_folder_changes_nothing() {
        let mut tree = FileTree::default();
        tree.add_ignored(["target/"]);
        let entry = || {
            vec![FolderEntry {
                name: "out".into(),
                is_dir: false,
            }]
        };
        tree.fill("target", entry());
        tree.fill("target", entry());
        tree.fill("missing", entry());
        let expanded: HashSet<String> = ["target".to_owned()].into();
        assert_eq!(names(&tree.rows(&expanded)), ["target/", "  out"]);
    }

    #[test]
    fn ancestors_reveal_a_file() {
        assert_eq!(ancestors("a/b/c.rs"), ["a", "a/b"]);
        assert!(ancestors("top.rs").is_empty());
    }
}
