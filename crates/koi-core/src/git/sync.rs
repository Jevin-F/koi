//! Committing, and pushing to the branch's remote.

use std::path::Path;

use super::{Error, Result, git};

/// How the current branch compares with the branch it tracks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tracking {
    /// Such as `origin/feature`.
    pub upstream: String,
    /// Local commits not pushed yet.
    pub ahead: u32,
    /// Remote commits not pulled yet.
    pub behind: u32,
}

/// The upstream of the current branch in `dir`; `None` if it has none.
pub fn tracking(dir: &Path) -> Result<Option<Tracking>> {
    let upstream = match git(
        dir,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    ) {
        Ok(upstream) => upstream.trim().to_owned(),
        Err(Error::Failed { .. }) => return Ok(None),
        Err(error) => return Err(error),
    };
    let counts = git(
        dir,
        &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"],
    )?;
    let mut numbers = counts
        .split_whitespace()
        .filter_map(|number| number.parse().ok());
    let (Some(behind), Some(ahead)) = (numbers.next(), numbers.next()) else {
        return Ok(None);
    };
    Ok(Some(Tracking {
        upstream,
        ahead,
        behind,
    }))
}

/// Stages every change in the working tree at `dir` and commits it.
pub fn commit_all(dir: &Path, message: &str) -> Result<()> {
    git(dir, &["add", "--all"])?;
    git(dir, &["commit", "--message", message]).map(drop)
}

/// Pushes the current branch and returns the upstream it went to. A branch
/// without an upstream is pushed to `origin`, or to the only remote, and
/// tracks it from then on.
pub fn push(dir: &Path) -> Result<String> {
    if let Some(tracking) = tracking(dir)? {
        git(dir, &["push"])?;
        return Ok(tracking.upstream);
    }
    let remotes = git(dir, &["remote"])?;
    let remote = pick_remote(&remotes).ok_or(Error::NoRemote)?;
    git(dir, &["push", "--set-upstream", remote, "HEAD"])?;
    Ok(tracking(dir)?.map_or_else(|| remote.to_owned(), |tracking| tracking.upstream))
}

/// `origin` if it exists, otherwise the only remote.
fn pick_remote(remotes: &str) -> Option<&str> {
    let names: Vec<&str> = remotes
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();
    match names.as_slice() {
        [only] => Some(only),
        _ => names.into_iter().find(|&name| name == "origin"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::changes;
    use crate::git::testing::init_repo;
    use crate::test_support::TempDir;

    #[test]
    fn picks_origin_or_the_only_remote() {
        assert_eq!(pick_remote("origin\nfork\n"), Some("origin"));
        assert_eq!(pick_remote("upstream\n"), Some("upstream"));
        assert_eq!(pick_remote("a\nb\n"), None);
        assert_eq!(pick_remote(""), None);
    }

    #[test]
    fn commits_and_pushes_to_a_new_upstream() {
        let temp = TempDir::new();
        let repo = temp.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        init_repo(&repo);
        assert!(matches!(push(&repo), Err(Error::NoRemote)));

        git(temp.path(), &["init", "-q", "--bare", "remote.git"]).unwrap();
        git(&repo, &["remote", "add", "origin", "../remote.git"]).unwrap();
        assert_eq!(tracking(&repo).unwrap(), None);

        std::fs::write(repo.join("notes.txt"), "draft\n").unwrap();
        commit_all(&repo, "Add notes").unwrap();
        assert!(changes(&repo).unwrap().is_empty());

        assert_eq!(push(&repo).unwrap(), "origin/main");
        let pushed = tracking(&repo).unwrap().unwrap();
        assert_eq!((pushed.ahead, pushed.behind), (0, 0));

        std::fs::write(repo.join("notes.txt"), "final\n").unwrap();
        commit_all(&repo, "Finish notes").unwrap();
        assert_eq!(tracking(&repo).unwrap().unwrap().ahead, 1);
        push(&repo).unwrap();
        assert_eq!(tracking(&repo).unwrap().unwrap().ahead, 0);
    }
}
