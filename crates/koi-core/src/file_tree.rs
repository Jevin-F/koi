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
}

#[derive(Default)]
struct Dir {
    dirs: BTreeMap<String, Dir>,
    files: Vec<String>,
}

/// Folders and files of a worktree.
#[derive(Default)]
pub struct FileTree {
    root: Dir,
}

impl FileTree {
    /// Builds the tree from paths relative to the root, such as `src/main.rs`.
    pub fn new<'a>(paths: impl IntoIterator<Item = &'a str>) -> Self {
        let mut root = Dir::default();
        for path in paths {
            let mut parts: Vec<&str> = path.split('/').collect();
            let Some(file) = parts.pop() else {
                continue;
            };
            let dir = parts.into_iter().fold(&mut root, |dir, part| {
                dir.dirs.entry(part.to_owned()).or_default()
            });
            dir.files.push(file.to_owned());
        }
        Self { root }
    }

    /// Visible rows: folders before files, each sorted ignoring case, with
    /// the contents of folders in `expanded` (by path) shown below them.
    pub fn rows(&self, expanded: &HashSet<String>) -> Vec<TreeRow> {
        let mut rows = Vec::new();
        push_rows(&self.root, "", 0, expanded, &mut rows);
        rows
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
        });
        if is_expanded {
            push_rows(child, &format!("{path}/"), depth + 1, expanded, rows);
        }
    }
    let mut files: Vec<&String> = dir.files.iter().collect();
    files.sort_by_key(|name| name.to_lowercase());
    rows.extend(files.into_iter().map(|name| TreeRow {
        depth,
        name: name.clone(),
        path: format!("{prefix}{name}"),
        is_dir: false,
        expanded: false,
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
    fn ancestors_reveal_a_file() {
        assert_eq!(ancestors("a/b/c.rs"), ["a", "a/b"]);
        assert!(ancestors("top.rs").is_empty());
    }
}
