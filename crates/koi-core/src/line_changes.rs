//! Which lines of a file differ from its last committed version, for marks
//! beside the editor's line numbers.

use std::ops::Range;

/// Most edits the line diff looks for. Past it the differing middle of the
/// file is marked as modified as a whole, so a rewritten file stays cheap.
const MAX_EDITS: usize = 2000;

/// How a run of lines changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineChangeKind {
    Added,
    Modified,
    /// Lines were removed just above the first line of the range, which is
    /// empty.
    Deleted,
}

/// A run of changed lines, as zero-based line numbers of the current text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineChange {
    pub kind: LineChangeKind,
    pub lines: Range<usize>,
}

/// The lines of `text` that differ from `base`. A missing final newline is
/// not a change.
pub fn line_changes(base: &str, text: &str) -> Vec<LineChange> {
    let old: Vec<&str> = base.lines().collect();
    let new: Vec<&str> = text.lines().collect();
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let old = &old[prefix..old.len() - suffix];
    let new = &new[prefix..new.len() - suffix];
    let edits = shortest_edits(old, new).unwrap_or_else(|| {
        let mut edits: Vec<Edit> = (0..old.len()).map(|x| Edit::delete(x, 0)).collect();
        edits.extend((0..new.len()).map(|y| Edit::insert(old.len(), y)));
        edits
    });
    group(&edits, prefix)
}

/// One step off the diagonal of the edit graph, at old line `x` and new line `y`.
#[derive(Clone, Copy, Debug)]
struct Edit {
    x: usize,
    y: usize,
    insert: bool,
}

impl Edit {
    fn delete(x: usize, y: usize) -> Self {
        Self {
            x,
            y,
            insert: false,
        }
    }

    fn insert(x: usize, y: usize) -> Self {
        Self { x, y, insert: true }
    }
}

/// The fewest deletions and insertions turning `old` into `new` (Myers'
/// algorithm), in order, or `None` when there are more than [`MAX_EDITS`].
fn shortest_edits(old: &[&str], new: &[&str]) -> Option<Vec<Edit>> {
    let (n, m) = (old.len() as isize, new.len() as isize);
    // `rounds[d][k + d]` is the furthest `x` on diagonal `k = x - y` after `d` edits.
    let mut rounds: Vec<Vec<isize>> = Vec::new();
    for d in 0..=(MAX_EDITS as isize).min(n + m) {
        let mut round = vec![0; 2 * d as usize + 1];
        for k in (-d..=d).step_by(2) {
            let mut x = match rounds.last() {
                None => 0,
                Some(previous) => {
                    let at = |k: isize| previous[(k + d - 1) as usize];
                    if k == -d || (k != d && at(k - 1) < at(k + 1)) {
                        at(k + 1)
                    } else {
                        at(k - 1) + 1
                    }
                }
            };
            let mut y = x - k;
            while x < n && y < m && old[x as usize] == new[y as usize] {
                x += 1;
                y += 1;
            }
            round[(k + d) as usize] = x;
            if x >= n && y >= m {
                rounds.push(round);
                return Some(backtrack(&rounds, n, m));
            }
        }
        rounds.push(round);
    }
    None
}

/// Walks the rounds back from the end to the edits that reached it.
fn backtrack(rounds: &[Vec<isize>], n: isize, m: isize) -> Vec<Edit> {
    let (mut x, mut y) = (n, m);
    let mut edits = Vec::new();
    for d in (1..rounds.len() as isize).rev() {
        let previous = &rounds[d as usize - 1];
        let at = |k: isize| previous[(k + d - 1) as usize];
        let k = x - y;
        let inserted = k == -d || (k != d && at(k - 1) < at(k + 1));
        let previous_k = if inserted { k + 1 } else { k - 1 };
        let previous_x = at(previous_k);
        let previous_y = previous_x - previous_k;
        edits.push(if inserted {
            Edit::insert(previous_x as usize, previous_y as usize)
        } else {
            Edit::delete(previous_x as usize, previous_y as usize)
        });
        (x, y) = (previous_x, previous_y);
    }
    edits.reverse();
    edits
}

/// Joins adjacent edits into runs of changed lines, shifted by `offset`.
fn group(edits: &[Edit], offset: usize) -> Vec<LineChange> {
    let mut changes = Vec::new();
    let mut edits = edits.iter().peekable();
    while let Some(first) = edits.next() {
        let (mut x, mut y) = (first.x, first.y);
        let (mut deleted, mut inserted) = (0, 0);
        let mut step = |edit: &Edit, x: &mut usize, y: &mut usize| {
            if edit.insert {
                inserted += 1;
                *y += 1;
            } else {
                deleted += 1;
                *x += 1;
            }
        };
        step(first, &mut x, &mut y);
        while let Some(edit) = edits.next_if(|edit| edit.x == x && edit.y == y) {
            step(edit, &mut x, &mut y);
        }
        let start = offset + first.y;
        let kind = match (deleted, inserted) {
            (0, _) => LineChangeKind::Added,
            (_, 0) => LineChangeKind::Deleted,
            _ => LineChangeKind::Modified,
        };
        changes.push(LineChange {
            kind,
            lines: start..start + inserted,
        });
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::{LineChange, LineChangeKind, MAX_EDITS, line_changes};

    fn change(kind: LineChangeKind, lines: std::ops::Range<usize>) -> LineChange {
        LineChange { kind, lines }
    }

    #[test]
    fn unchanged_text_has_no_marks() {
        assert!(line_changes("a\nb\n", "a\nb\n").is_empty());
        assert!(line_changes("a\nb\n", "a\nb").is_empty());
    }

    #[test]
    fn added_modified_and_deleted_lines_are_told_apart() {
        let base = "one\ntwo\nthree\nfour\nfive\n";
        let text = "zero\none\nTWO\nthree\nfive\n";
        assert_eq!(
            line_changes(base, text),
            [
                change(LineChangeKind::Added, 0..1),
                change(LineChangeKind::Modified, 2..3),
                change(LineChangeKind::Deleted, 4..4),
            ]
        );
    }

    #[test]
    fn edits_at_the_ends_are_found() {
        assert_eq!(
            line_changes("a\nb\n", "a\nb\nc\n"),
            [change(LineChangeKind::Added, 2..3)]
        );
        assert_eq!(
            line_changes("a\nb\nc\n", "b\nc\n"),
            [change(LineChangeKind::Deleted, 0..0)]
        );
    }

    #[test]
    fn a_new_or_emptied_file_changes_throughout() {
        assert_eq!(
            line_changes("", "a\nb\n"),
            [change(LineChangeKind::Added, 0..2)]
        );
        assert_eq!(
            line_changes("a\nb\n", ""),
            [change(LineChangeKind::Deleted, 0..0)]
        );
    }

    #[test]
    fn a_rewritten_file_is_marked_as_one_modified_run() {
        let lines = MAX_EDITS;
        let base: String = (0..lines).map(|ix| format!("old {ix}\n")).collect();
        let text: String = (0..lines).map(|ix| format!("new {ix}\n")).collect();
        assert_eq!(
            line_changes(&base, &text),
            [change(LineChangeKind::Modified, 0..lines)]
        );
    }
}
