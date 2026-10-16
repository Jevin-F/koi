//! Working trees of a repository.

use std::path::{Path, PathBuf};

use super::{Result, branch::branch_exists, git};

/// A working tree of a repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    /// Checked-out branch; `None` for a detached HEAD.
    pub branch: Option<String>,
    /// The repository's original working tree, which cannot be removed.
    pub is_main: bool,
}

/// All working trees of the repository at `repo`, the main one first.
pub fn worktrees(repo: &Path) -> Result<Vec<Worktree>> {
    git(repo, &["worktree", "list", "--porcelain"]).map(|output| parse_worktree_list(&output))
}

/// Parses `git worktree list --porcelain`, skipping bare and pruned entries.
fn parse_worktree_list(output: &str) -> Vec<Worktree> {
    let mut worktrees = Vec::new();
    for record in output.split("\n\n") {
        let mut path = None;
        let mut branch = None;
        let mut usable = true;
        for line in record.lines() {
            let (key, value) = line.split_once(' ').unwrap_or((line, ""));
            match key {
                "worktree" => path = Some(PathBuf::from(value)),
                "branch" => branch = Some(value.trim_start_matches("refs/heads/").to_owned()),
                "bare" | "prunable" => usable = false,
                _ => {}
            }
        }
        if let (Some(path), true) = (path, usable) {
            worktrees.push(Worktree {
                path,
                branch,
                is_main: worktrees.is_empty(),
            });
        }
    }
    worktrees
}

/// Creates a working tree at `path` for `branch`. An existing local branch
/// is checked out; otherwise the branch is created from `start`, a
/// remote-tracking branch it will then track, or from the current HEAD.
pub fn add_worktree(repo: &Path, path: &Path, branch: &str, start: Option<&str>) -> Result<()> {
    let path = path.to_string_lossy();
    if branch_exists(repo, branch)? {
        git(repo, &["worktree", "add", &path, branch])?;
    } else {
        let mut args = vec!["worktree", "add", "-b", branch, &path];
        args.extend(start);
        git(repo, &args)?;
    }
    Ok(())
}

/// Removes the working tree at `path`. Without `force`, git refuses to remove
/// a tree with uncommitted changes. The branch is kept.
pub fn remove_worktree(repo: &Path, path: &Path, force: bool) -> Result<()> {
    let path = path.to_string_lossy();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&path);
    git(repo, &args).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{common_dir, has_uncommitted_changes, repo_root, testing::init_repo};
    use crate::test_support::TempDir;

    #[test]
    fn parses_worktree_list() {
        let output = "worktree /repo\nHEAD abc\nbranch refs/heads/main\n\n\
                      worktree /trees/feature\nHEAD def\nbranch refs/heads/feature/login\n\n\
                      worktree /trees/detached\nHEAD 123\ndetached\n\n\
                      worktree /trees/gone\nHEAD 456\nbranch refs/heads/old\nprunable gitdir file points to non-existent location\n";
        assert_eq!(
            parse_worktree_list(output),
            [
                Worktree {
                    path: "/repo".into(),
                    branch: Some("main".into()),
                    is_main: true,
                },
                Worktree {
                    path: "/trees/feature".into(),
                    branch: Some("feature/login".into()),
                    is_main: false,
                },
                Worktree {
                    path: "/trees/detached".into(),
                    branch: None,
                    is_main: false,
                },
            ]
        );
    }

    #[test]
    fn worktree_lifecycle() {
        let temp = TempDir::new();
        let repo = temp.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        init_repo(&repo);
        assert_eq!(repo_root(&repo).unwrap(), repo);

        let tree = temp.path().join("trees/feature");
        add_worktree(&repo, &tree, "feature", None).unwrap();
        assert_eq!(common_dir(&tree).unwrap(), repo.join(".git"));
        let trees = worktrees(&repo).unwrap();
        assert_eq!(trees.len(), 2);
        assert_eq!(trees[1].path, tree);
        assert_eq!(trees[1].branch.as_deref(), Some("feature"));

        assert!(!has_uncommitted_changes(&tree).unwrap());
        std::fs::write(tree.join("notes.txt"), "draft").unwrap();
        assert!(has_uncommitted_changes(&tree).unwrap());
        assert!(remove_worktree(&repo, &tree, false).is_err());
        remove_worktree(&repo, &tree, true).unwrap();
        assert_eq!(worktrees(&repo).unwrap().len(), 1);

        // The branch survives, so it can be checked out again.
        add_worktree(&repo, &tree, "feature", None).unwrap();
        assert_eq!(worktrees(&repo).unwrap().len(), 2);
    }
}
