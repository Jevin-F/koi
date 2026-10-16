//! Fuzzy matching for quick search.
//!
//! A query is split into words; every word must appear in the candidate as a
//! subsequence, in any word order and ignoring case. Matches at the start of
//! words and runs of consecutive characters score higher; gaps cost points.

/// How well a candidate matches, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FuzzyMatch {
    pub score: i64,
    /// Byte offsets of the matched characters, ascending.
    pub positions: Vec<usize>,
}

const WORD_START_BONUS: i64 = 8;
const CONSECUTIVE_BONUS: i64 = 8;
const GAP_PENALTY: i64 = 3;

/// Matches `query` against `text`; an empty query matches everything.
pub fn fuzzy_match(query: &str, text: &str) -> Option<FuzzyMatch> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut result = FuzzyMatch {
        score: 0,
        positions: Vec::new(),
    };
    for word in query.split_whitespace() {
        let (score, positions) = best_word_match(word, &chars)?;
        result.score += score;
        result.positions.extend(positions);
    }
    result.positions.sort_unstable();
    result.positions.dedup();
    Some(result)
}

/// The best-scoring placement of `word` in `chars`, trying every place its
/// first character occurs and matching the rest greedily from there.
fn best_word_match(word: &str, chars: &[(usize, char)]) -> Option<(i64, Vec<usize>)> {
    let word: Vec<char> = word.chars().flat_map(char::to_lowercase).collect();
    let first = *word.first()?;
    (0..chars.len())
        .filter(|&start| lowercase_eq(chars[start].1, first))
        .filter_map(|start| match_from(&word, chars, start))
        .max_by_key(|(score, _)| *score)
}

fn match_from(word: &[char], chars: &[(usize, char)], start: usize) -> Option<(i64, Vec<usize>)> {
    let mut score = 0;
    let mut positions = Vec::with_capacity(word.len());
    let mut index = start;
    let mut previous: Option<usize> = None;
    for &wanted in word {
        let found = (index..chars.len()).find(|&i| lowercase_eq(chars[i].1, wanted))?;
        if is_word_start(chars, found) {
            score += WORD_START_BONUS;
        }
        match previous {
            Some(prev) if found == prev + 1 => score += CONSECUTIVE_BONUS,
            Some(prev) => score -= GAP_PENALTY * (found - prev - 1) as i64,
            None => {}
        }
        positions.push(chars[found].0);
        previous = Some(found);
        index = found + 1;
    }
    Some((score, positions))
}

fn lowercase_eq(c: char, lower: char) -> bool {
    c.to_lowercase().eq(std::iter::once(lower))
}

fn is_word_start(chars: &[(usize, char)], index: usize) -> bool {
    index == 0 || !chars[index - 1].1.is_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matched(query: &str, text: &str) -> Option<String> {
        let found = fuzzy_match(query, text)?;
        Some(
            found
                .positions
                .iter()
                .map(|&at| text[at..].chars().next().unwrap())
                .collect(),
        )
    }

    #[test]
    fn subsequence_ignoring_case() {
        assert_eq!(matched("CLD", "Claude Code").as_deref(), Some("Cld"));
        assert_eq!(matched("xyz", "Claude Code"), None);
    }

    #[test]
    fn words_match_in_any_order() {
        let text = "Claude Code · feat/login · koi";
        assert!(fuzzy_match("feat claude", text).is_some());
        assert!(fuzzy_match("feat codex", text).is_none());
    }

    #[test]
    fn prefers_word_starts_over_the_first_occurrence() {
        assert_eq!(matched("fl", "self feat/login").as_deref(), Some("fl"));
        let positions = fuzzy_match("fl", "self feat/login").unwrap().positions;
        assert_eq!(positions, [5, 10]);
    }

    #[test]
    fn tighter_matches_score_higher() {
        let tight = fuzzy_match("login", "feat/login").unwrap().score;
        let loose = fuzzy_match("login", "l-o-g-i-n").unwrap().score;
        assert!(tight > loose);
    }

    #[test]
    fn positions_are_byte_offsets() {
        assert_eq!(fuzzy_match("修", "修复 bug").unwrap().positions, [0]);
        assert_eq!(fuzzy_match("b", "修复 bug").unwrap().positions, [7]);
    }

    #[test]
    fn empty_query_matches_everything() {
        assert_eq!(fuzzy_match("  ", "anything").unwrap().score, 0);
    }
}
