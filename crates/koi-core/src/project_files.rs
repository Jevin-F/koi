//! The files shown for a project directory: in a git repository, those git
//! tracks or would offer to track; in a plain folder, every file found.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;

use crate::file_tree::{FileTree, FolderEntry};
use crate::git;

/// Most files listed for a plain folder, so that adding a very large one
/// cannot stall the file list.
pub const MAX_FOLDER_FILES: usize = 20_000;

/// Files under `dir` as paths relative to it with `/` separators, in path
/// order.
pub fn files(dir: &Path) -> Result<Vec<String>, String> {
    if git::is_repository(dir) {
        git::files(dir).map_err(|error| error.to_string())
    } else {
        folder_files(dir, MAX_FOLDER_FILES).map_err(|error| error.to_string())
    }
}

/// The tree of `dir`, also showing what git ignores, with the contents of
/// ignored folders in `expanded` read from disk. Ignored folders that stay
/// collapsed are never walked.
pub fn tree(dir: &Path, expanded: &HashSet<String>) -> Result<FileTree, String> {
    let mut tree = FileTree::new(files(dir)?.iter().map(String::as_str));
    // A failed listing of ignored entries only leaves them out.
    if git::is_repository(dir)
        && let Ok(ignored) = git::ignored(dir)
    {
        tree.add_ignored(ignored.iter().map(String::as_str));
    }
    loop {
        let unread = tree.unread(expanded);
        if unread.is_empty() {
            return Ok(tree);
        }
        for folder in unread {
            let entries = read_folder(dir, &folder);
            tree.fill(&folder, entries);
        }
    }
}

/// The entries directly inside `folder` of `dir`, without `.git`. An
/// unreadable folder reads as empty.
pub fn read_folder(dir: &Path, folder: &str) -> Vec<FolderEntry> {
    let Ok(entries) = fs::read_dir(dir.join(folder)) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            // Following a link here is safe: only folders the user opens are read.
            let is_dir = entry.path().is_dir();
            (name != ".git").then_some(FolderEntry { name, is_dir })
        })
        .collect()
}

/// Walks `dir` for at most `limit` files. Symbolic links to folders are not
/// followed, which keeps the walk inside `dir` and free of cycles, and `.git`
/// folders of nested repositories are skipped. Names that are not valid
/// UTF-8 are left out.
fn folder_files(dir: &Path, limit: usize) -> io::Result<Vec<String>> {
    let mut files = Vec::new();
    let mut pending = vec![String::new()];
    'walk: while let Some(prefix) = pending.pop() {
        let entries = match fs::read_dir(dir.join(&prefix)) {
            Ok(entries) => entries,
            Err(error) if prefix.is_empty() => return Err(error),
            // An unreadable subfolder only hides its own files.
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let (Ok(name), Ok(kind)) = (entry.file_name().into_string(), entry.file_type()) else {
                continue;
            };
            let path = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            if kind.is_dir() {
                if entry.file_name() != ".git" {
                    pending.push(path);
                }
            } else if kind.is_file() || entry.path().is_file() {
                files.push(path);
                if files.len() >= limit {
                    break 'walk;
                }
            }
        }
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::testing::init_repo;
    use crate::test_support::TempDir;

    fn write(root: &Path, path: &str) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }

    #[test]
    fn lists_every_file_of_a_plain_folder() {
        let temp = TempDir::new();
        let root = temp.path();
        for path in ["b.md", "a/z.txt", "a/b/c.rs", ".env", "nested/.git/HEAD"] {
            write(root, path);
        }
        assert_eq!(
            files(root).unwrap(),
            [".env", "a/b/c.rs", "a/z.txt", "b.md"]
        );
    }

    #[test]
    fn does_not_follow_folder_links() {
        let temp = TempDir::new();
        let root = temp.path();
        write(root, "real/file.txt");
        std::os::unix::fs::symlink(root.join("real"), root.join("link")).unwrap();
        std::os::unix::fs::symlink(root.join("real/file.txt"), root.join("file-link")).unwrap();
        assert_eq!(files(root).unwrap(), ["file-link", "real/file.txt"]);
    }

    #[test]
    fn stops_at_the_limit() {
        let temp = TempDir::new();
        for ix in 0..5 {
            write(temp.path(), &format!("{ix}.txt"));
        }
        assert_eq!(folder_files(temp.path(), 3).unwrap().len(), 3);
    }

    #[test]
    fn a_missing_folder_is_an_error() {
        let temp = TempDir::new();
        assert!(files(&temp.path().join("missing")).is_err());
    }

    #[test]
    fn the_tree_reads_only_expanded_ignored_folders() {
        let temp = TempDir::new();
        let root = temp.path();
        init_repo(root);
        fs::write(root.join(".gitignore"), "node_modules/\n.env\n").unwrap();
        for path in [
            "src/main.rs",
            ".env",
            "node_modules/react/index.js",
            "node_modules/a.txt",
        ] {
            write(root, path);
        }
        let mut expanded = HashSet::new();
        let names = |tree: &FileTree, expanded: &HashSet<String>| -> Vec<(String, bool)> {
            tree.rows(expanded)
                .into_iter()
                .map(|row| (row.path, row.ignored))
                .collect()
        };
        let collapsed = tree(root, &expanded).unwrap();
        assert_eq!(
            names(&collapsed, &expanded),
            [
                ("node_modules".to_owned(), true),
                ("src".to_owned(), false),
                (".env".to_owned(), true),
                (".gitignore".to_owned(), false),
            ]
        );

        expanded.extend(["node_modules".to_owned(), "node_modules/react".to_owned()]);
        let open = tree(root, &expanded).unwrap();
        assert!(open.unread(&expanded).is_empty());
        let paths: Vec<String> = names(&open, &expanded)
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        assert_eq!(
            paths[..4],
            [
                "node_modules",
                "node_modules/react",
                "node_modules/react/index.js",
                "node_modules/a.txt"
            ]
        );
    }

    #[test]
    fn reading_a_folder_skips_git_metadata() {
        let temp = TempDir::new();
        write(temp.path(), "lib/.git/HEAD");
        write(temp.path(), "lib/x.rs");
        let entries = read_folder(temp.path(), "lib");
        assert_eq!(
            entries,
            [FolderEntry {
                name: "x.rs".into(),
                is_dir: false
            }]
        );
    }

    #[test]
    fn leaves_out_files_git_ignores_in_a_repository() {
        let temp = TempDir::new();
        let root = temp.path();
        init_repo(root);
        fs::write(root.join(".gitignore"), "target/\n").unwrap();
        write(root, "src/main.rs");
        write(root, "target/out");
        assert_eq!(files(root).unwrap(), [".gitignore", "src/main.rs"]);
    }
}
