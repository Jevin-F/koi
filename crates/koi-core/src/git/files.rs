//! Files of a working tree.

use std::path::Path;

use super::{Result, git};

/// Files in the working tree at `dir` that git tracks or would offer to
/// track, so ignored files such as build output are left out. Paths are
/// relative to `dir`, in path order.
pub fn files(dir: &Path) -> Result<Vec<String>> {
    let output = git(
        dir,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?;
    let mut files: Vec<String> = output
        .split('\0')
        .filter(|path| !path.is_empty())
        // Tracked files deleted from disk are still listed as cached.
        .filter(|path| dir.join(path).is_file())
        .map(str::to_owned)
        .collect();
    files.sort();
    files.dedup();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::testing::init_repo;
    use crate::test_support::TempDir;

    #[test]
    fn lists_tracked_and_untracked_files_but_not_ignored_ones() {
        let temp = TempDir::new();
        let repo = temp.path();
        init_repo(repo);
        std::fs::write(repo.join(".gitignore"), "target/\n").unwrap();
        std::fs::write(repo.join("gone.txt"), "x").unwrap();
        git(repo, &["add", "."]).unwrap();
        git(repo, &["commit", "-q", "-m", "files"]).unwrap();
        std::fs::remove_file(repo.join("gone.txt")).unwrap();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/new.rs"), "").unwrap();
        std::fs::create_dir_all(repo.join("target")).unwrap();
        std::fs::write(repo.join("target/out"), "").unwrap();

        assert_eq!(files(repo).unwrap(), [".gitignore", "src/new.rs"]);
    }
}
