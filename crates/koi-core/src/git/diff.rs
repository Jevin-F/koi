//! Differences of one file against the last commit.

use std::path::Path;

use super::{ChangeKind, FileChange, Result, git, git_accepting};

/// What a line of a diff shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    /// The start of a hunk, such as `@@ -1,3 +1,4 @@ fn main`.
    Hunk,
    Context,
    Added,
    Removed,
    /// Information from git, such as "Binary files differ".
    Note,
}

/// A line of a diff with its numbers in the old and new file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    /// The line without its leading `+`, `-` or space.
    pub text: String,
    pub old: Option<u32>,
    pub new: Option<u32>,
}

/// The diff of `change` in the working tree at `dir` against `HEAD`,
/// including staged and unstaged edits.
pub fn file_diff(dir: &Path, change: &FileChange) -> Result<Vec<DiffLine>> {
    let output = match (change.kind, &change.original) {
        // `--no-index` exits with 1 when the files differ, which they always do here.
        (ChangeKind::Untracked, _) => {
            let args = [
                "diff",
                "--no-index",
                "--",
                "/dev/null",
                change.path.as_str(),
            ];
            git_accepting(dir, &args, &[1])?
        }
        (_, Some(original)) => git(dir, &["diff", "-M", "HEAD", "--", original, &change.path])?,
        _ => git(dir, &["diff", "HEAD", "--", &change.path])?,
    };
    Ok(parse_diff(&output))
}

/// Parses unified diff output, skipping the file headers.
fn parse_diff(output: &str) -> Vec<DiffLine> {
    let mut lines = Vec::new();
    let (mut old, mut new) = (0, 0);
    let mut in_hunk = false;
    for line in output.lines() {
        let line_of = |kind, text: &str, old, new| DiffLine {
            kind,
            text: text.to_owned(),
            old,
            new,
        };
        if let Some((old_start, new_start)) = hunk_starts(line) {
            (old, new, in_hunk) = (old_start, new_start, true);
            lines.push(line_of(LineKind::Hunk, line, None, None));
        } else if line.starts_with("diff ") {
            in_hunk = false;
        } else if !in_hunk {
            if line.starts_with("Binary files ") {
                lines.push(line_of(LineKind::Note, line, None, None));
            }
        } else if let Some(text) = line.strip_prefix('+') {
            lines.push(line_of(LineKind::Added, text, None, Some(new)));
            new += 1;
        } else if let Some(text) = line.strip_prefix('-') {
            lines.push(line_of(LineKind::Removed, text, Some(old), None));
            old += 1;
        } else if let Some(text) = line.strip_prefix('\\') {
            lines.push(line_of(LineKind::Note, text.trim(), None, None));
        } else {
            let text = line.strip_prefix(' ').unwrap_or(line);
            lines.push(line_of(LineKind::Context, text, Some(old), Some(new)));
            old += 1;
            new += 1;
        }
    }
    lines
}

/// The first old and new line numbers of a hunk header `@@ -a,b +c,d @@`.
fn hunk_starts(line: &str) -> Option<(u32, u32)> {
    let ranges = line.strip_prefix("@@ ")?.split(" @@").next()?;
    let (old, new) = ranges.split_once(' ')?;
    let start = |range: &str, sign: char| -> Option<u32> {
        range.strip_prefix(sign)?.split(',').next()?.parse().ok()
    };
    Some((start(old, '-')?, start(new, '+')?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::changes;
    use crate::git::testing::init_repo;
    use crate::test_support::TempDir;

    fn summary(lines: &[DiffLine]) -> Vec<(LineKind, &str, Option<u32>, Option<u32>)> {
        lines
            .iter()
            .map(|line| (line.kind, line.text.as_str(), line.old, line.new))
            .collect()
    }

    #[test]
    fn parses_hunks_with_line_numbers() {
        let output = "diff --git a/f b/f\nindex 1..2 100644\n--- a/f\n+++ b/f\n\
                      @@ -3,3 +3,3 @@ fn main\n keep\n-old\n+new\n\\ No newline at end of file\n";
        assert_eq!(
            summary(&parse_diff(output)),
            [
                (LineKind::Hunk, "@@ -3,3 +3,3 @@ fn main", None, None),
                (LineKind::Context, "keep", Some(3), Some(3)),
                (LineKind::Removed, "old", Some(4), None),
                (LineKind::Added, "new", None, Some(4)),
                (LineKind::Note, "No newline at end of file", None, None),
            ]
        );
    }

    #[test]
    fn reports_binary_files() {
        let output = "diff --git a/i.png b/i.png\nBinary files a/i.png and b/i.png differ\n";
        assert_eq!(parse_diff(output)[0].kind, LineKind::Note);
    }

    #[test]
    fn reads_hunk_headers() {
        assert_eq!(hunk_starts("@@ -12,7 +13,9 @@ impl X"), Some((12, 13)));
        assert_eq!(hunk_starts("@@ -0,0 +1 @@"), Some((0, 1)));
        assert_eq!(hunk_starts("+@@ not a header"), None);
    }

    #[test]
    fn diffs_modified_and_untracked_files() {
        let temp = TempDir::new();
        let repo = temp.path();
        init_repo(repo);
        std::fs::write(repo.join("a.txt"), "one\ntwo\n").unwrap();
        git(repo, &["add", "a.txt"]).unwrap();
        git(repo, &["commit", "-q", "-m", "add a"]).unwrap();
        std::fs::write(repo.join("a.txt"), "one\nTWO\n").unwrap();
        std::fs::write(repo.join("b.txt"), "fresh\n").unwrap();

        let found = changes(repo).unwrap();
        let modified = file_diff(repo, &found[0]).unwrap();
        assert!(summary(&modified).contains(&(LineKind::Added, "TWO", None, Some(2))));
        let untracked = file_diff(repo, &found[1]).unwrap();
        assert!(summary(&untracked).contains(&(LineKind::Added, "fresh", None, Some(1))));
    }
}
