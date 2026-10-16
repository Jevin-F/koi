//! Git operations, performed by running the `git` command line.

mod branch;
mod diff;
mod files;
mod status;
mod sync;
mod worktree;

pub use branch::{Branch, branches, is_valid_branch_name};
pub use diff::{DiffLine, LineKind, file_diff};
pub use files::files;
pub use status::{ChangeKind, FileChange, changes, has_uncommitted_changes};
pub use sync::{Tracking, commit_all, push, tracking};
pub use worktree::{Worktree, add_worktree, remove_worktree, worktrees};

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A failed git operation.
#[derive(Debug)]
pub enum Error {
    /// `git` could not be started.
    Io(io::Error),
    /// `git` ran and reported an error.
    Failed { command: String, message: String },
    /// There is no remote to push to.
    NoRemote,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "could not run git: {error}"),
            Self::Failed { command, message } => write!(f, "git {command}: {message}"),
            Self::NoRemote => write!(f, "the repository has no remote to push to"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Runs git in `dir` and returns its standard output.
fn git(dir: &Path, args: &[&str]) -> Result<String> {
    git_accepting(dir, args, &[])
}

/// Like [`git`], also treating the exit codes in `accepted` as success, for
/// commands that use them to report a result rather than a failure.
fn git_accepting(dir: &Path, args: &[&str], accepted: &[i32]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(Error::Io)?;
    let code = output.status.code();
    if output.status.success() || code.is_some_and(|code| accepted.contains(&code)) {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(Error::Failed {
            command: args.join(" "),
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        })
    }
}

/// The top-level directory of the working tree containing `path`.
pub fn repo_root(path: &Path) -> Result<PathBuf> {
    let root = git(path, &["rev-parse", "--show-toplevel"])?;
    Ok(PathBuf::from(root.trim_end()))
}

/// The directory holding the repository's shared git data (usually
/// `<root>/.git`), the same for every worktree.
pub fn common_dir(repo: &Path) -> Result<PathBuf> {
    let dir = git(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    Ok(PathBuf::from(dir.trim_end()))
}

#[cfg(test)]
pub(crate) mod testing {
    use std::path::Path;

    use super::git;

    /// Initialises a repository with one empty commit on `main`.
    pub fn init_repo(dir: &Path) {
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.email", "test@example.com"],
            &["config", "user.name", "Test"],
            &["config", "commit.gpgsign", "false"],
            &["commit", "-q", "--allow-empty", "-m", "initial"],
        ] {
            git(dir, args).expect("set up repository");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn errors_outside_a_repository() {
        let temp = TempDir::new();
        assert!(matches!(repo_root(temp.path()), Err(Error::Failed { .. })));
    }
}
