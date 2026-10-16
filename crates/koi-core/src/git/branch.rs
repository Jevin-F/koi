//! Branches.

use std::path::Path;

use super::{Error, Result, git};

/// Whether `name` is acceptable as a new branch name.
pub fn is_valid_branch_name(repo: &Path, name: &str) -> Result<bool> {
    match git(repo, &["check-ref-format", "--branch", name]) {
        Ok(_) => Ok(true),
        Err(Error::Failed { .. }) => Ok(false),
        Err(error) => Err(error),
    }
}

pub(super) fn branch_exists(repo: &Path, branch: &str) -> Result<bool> {
    let reference = format!("refs/heads/{branch}");
    match git(repo, &["show-ref", "--verify", "--quiet", &reference]) {
        Ok(_) => Ok(true),
        Err(Error::Failed { .. }) => Ok(false),
        Err(error) => Err(error),
    }
}

/// A branch that can be checked out in a new worktree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Branch {
    /// Local branch name.
    pub name: String,
    /// Remote-tracking branch to start from when there is no local branch
    /// yet, such as `origin/feature`.
    pub remote: Option<String>,
}

/// Local branches, then branches that exist only on a remote, each group
/// most recently committed first.
pub fn branches(repo: &Path) -> Result<Vec<Branch>> {
    let output = git(
        repo,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname)",
            "refs/heads",
            "refs/remotes",
        ],
    )?;
    Ok(parse_branches(&output))
}

fn parse_branches(output: &str) -> Vec<Branch> {
    let mut branches = Vec::new();
    let mut remote_only = Vec::new();
    for line in output.lines() {
        if let Some(name) = line.strip_prefix("refs/heads/") {
            branches.push(Branch {
                name: name.to_owned(),
                remote: None,
            });
        } else if let Some(remote) = line.strip_prefix("refs/remotes/")
            && let Some((_, name)) = remote.split_once('/')
            && name != "HEAD"
        {
            remote_only.push(Branch {
                name: name.to_owned(),
                remote: Some(remote.to_owned()),
            });
        }
    }
    for branch in remote_only {
        if branches.iter().all(|known| known.name != branch.name) {
            branches.push(branch);
        }
    }
    branches
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::add_worktree;
    use crate::git::testing::init_repo;
    use crate::test_support::TempDir;

    #[test]
    fn parses_branch_refs() {
        let output = "refs/heads/main\nrefs/remotes/origin/HEAD\nrefs/remotes/origin/main\n\
                      refs/remotes/origin/feature/x\nrefs/remotes/fork/feature/x\nrefs/heads/dev\n";
        assert_eq!(
            parse_branches(output),
            [
                Branch {
                    name: "main".into(),
                    remote: None
                },
                Branch {
                    name: "dev".into(),
                    remote: None
                },
                Branch {
                    name: "feature/x".into(),
                    remote: Some("origin/feature/x".into())
                },
            ]
        );
    }

    #[test]
    fn worktree_from_a_remote_branch_tracks_it() {
        let temp = TempDir::new();
        let origin = temp.path().join("origin");
        std::fs::create_dir(&origin).unwrap();
        init_repo(&origin);
        git(&origin, &["branch", "feature/x"]).unwrap();
        git(temp.path(), &["clone", "-q", "origin", "clone"]).unwrap();
        let repo = temp.path().join("clone");

        let remote = branches(&repo)
            .unwrap()
            .into_iter()
            .find(|b| b.name == "feature/x")
            .unwrap();
        assert_eq!(remote.remote.as_deref(), Some("origin/feature/x"));

        let tree = temp.path().join("tree");
        add_worktree(&repo, &tree, &remote.name, remote.remote.as_deref()).unwrap();
        let upstream = git(
            &repo,
            &["rev-parse", "--abbrev-ref", "feature/x@{upstream}"],
        )
        .unwrap();
        assert_eq!(upstream.trim(), "origin/feature/x");
    }

    #[test]
    fn validates_branch_names() {
        let temp = TempDir::new();
        init_repo(temp.path());
        assert!(is_valid_branch_name(temp.path(), "feature/login").unwrap());
        assert!(!is_valid_branch_name(temp.path(), "bad name").unwrap());
        assert!(!is_valid_branch_name(temp.path(), "").unwrap());
    }
}
