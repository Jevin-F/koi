//! The files shown for a project directory: in a git repository, those git
//! tracks or would offer to track; in a plain folder, every file found.

use std::fs;
use std::io;
use std::path::Path;

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
