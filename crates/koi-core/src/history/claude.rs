//! Claude Code: `~/.claude/projects/<cwd with every non-alphanumeric
//! character replaced by '-'>/<session id>.jsonl`.

use std::path::Path;

use super::{PastSession, content_text, jsonl_files, lines, one_line, parse_if};

pub(super) fn sessions(home: &Path, cwd: &Path) -> Vec<PastSession> {
    let encoded: String = cwd
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    jsonl_files(&home.join(".claude/projects").join(encoded))
        .into_iter()
        .filter_map(|(path, updated)| {
            let id = path.file_stem()?.to_str()?.to_owned();
            let title = title(&path, cwd)?;
            Some(PastSession {
                title,
                updated,
                resume_args: vec!["--resume".to_owned(), id],
            })
        })
        .collect()
}

/// The generated title, else the last prompt, else the first prompt.
/// `None` if the file belongs to another directory or has no prompt.
fn title(path: &Path, cwd: &Path) -> Option<String> {
    let (mut generated, mut last_prompt, mut first_prompt) = (None, None, None);
    for line in lines(path) {
        if let Some(entry) = parse_if(&line, "\"cwd\"")
            && entry["cwd"]
                .as_str()
                .is_some_and(|dir| Path::new(dir) != cwd)
        {
            return None;
        }
        if let Some(entry) = parse_if(&line, "\"ai-title\"") {
            generated = entry["aiTitle"].as_str().and_then(one_line).or(generated);
        } else if let Some(entry) = parse_if(&line, "\"last-prompt\"") {
            last_prompt = entry["lastPrompt"]
                .as_str()
                .and_then(one_line)
                .or(last_prompt);
        } else if first_prompt.is_none()
            && let Some(entry) = parse_if(&line, "\"type\":\"user\"")
        {
            first_prompt = content_text(&entry["message"]["content"])
                .filter(|text| !text.starts_with('<'))
                .and_then(|text| one_line(&text));
        }
    }
    generated.or(last_prompt).or(first_prompt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn write(home: &Path, dir: &str, id: &str, lines: &[&str]) {
        let dir = home.join(".claude/projects").join(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{id}.jsonl")), lines.join("\n")).unwrap();
    }

    #[test]
    fn lists_sessions_of_the_directory() {
        let home = TempDir::new();
        let cwd = "/code/my_app";
        let user =
            r#"{"type":"user","cwd":"/code/my_app","message":{"content":"fix the login bug"}}"#;
        write(home.path(), "-code-my-app", "first", &[user]);
        write(
            home.path(),
            "-code-my-app",
            "titled",
            &[
                user,
                r#"{"type":"ai-title","aiTitle":"Login fix","sessionId":"titled"}"#,
            ],
        );
        write(
            home.path(),
            "-code-my-app",
            "empty",
            &[r#"{"type":"system","cwd":"/code/my_app"}"#],
        );
        // Same encoded name, different directory.
        write(
            home.path(),
            "-code-my-app",
            "other",
            &[r#"{"type":"user","cwd":"/code/my.app","message":{"content":"x"}}"#],
        );

        let mut found = sessions(home.path(), Path::new(cwd));
        found.sort_by(|a, b| a.title.cmp(&b.title));
        let summary: Vec<(&str, &[String])> = found
            .iter()
            .map(|s| (s.title.as_str(), s.resume_args.as_slice()))
            .collect();
        assert_eq!(
            summary,
            [
                (
                    "Login fix",
                    &["--resume".to_owned(), "titled".to_owned()][..]
                ),
                (
                    "fix the login bug",
                    &["--resume".to_owned(), "first".to_owned()][..]
                ),
            ]
        );
    }
}
