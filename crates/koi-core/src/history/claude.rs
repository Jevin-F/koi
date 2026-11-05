//! Claude Code: `~/.claude/projects/<cwd with every non-alphanumeric
//! character replaced by '-'>/<session id>.jsonl`.

use std::path::{Path, PathBuf};

use super::{PastSession, content_text, jsonl_files, lines, one_line, parse_if};

pub(super) fn sessions(home: &Path, cwd: &Path) -> Vec<PastSession> {
    jsonl_files(&project_dir(home, cwd))
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

/// Where Claude Code keeps the conversations of `cwd`.
pub(super) fn project_dir(home: &Path, cwd: &Path) -> PathBuf {
    let encoded: String = cwd
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    home.join(".claude/projects").join(encoded)
}

/// The name the user gave the conversation with `/rename`, else its first
/// prompt. `None` if the file belongs to another directory or has no prompt.
///
/// Entries record the directory the agent is in at the time, which moves
/// when it changes directory in its shell, so only the first one tells
/// where the conversation started.
fn title(path: &Path, cwd: &Path) -> Option<String> {
    let (mut named, mut first_prompt) = (None, None);
    let mut dir_checked = false;
    for line in lines(path) {
        if !dir_checked
            && let Some(entry) = parse_if(&line, "\"cwd\"")
            && let Some(dir) = entry["cwd"].as_str()
        {
            if Path::new(dir) != cwd {
                return None;
            }
            dir_checked = true;
        }
        if let Some(entry) = parse_if(&line, "\"custom-title\"") {
            named = entry["customTitle"].as_str().and_then(one_line).or(named);
        } else if first_prompt.is_none()
            && let Some(entry) = parse_if(&line, "\"type\":\"user\"")
        {
            first_prompt = content_text(&entry["message"]["content"])
                .filter(|text| !text.starts_with('<'))
                .and_then(|text| one_line(&text));
        }
    }
    named.or(first_prompt)
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
        // Generated titles and later prompts do not name a conversation.
        write(
            home.path(),
            "-code-my-app",
            "first",
            &[
                user,
                r#"{"type":"ai-title","aiTitle":"Generated","sessionId":"first"}"#,
                r#"{"type":"last-prompt","lastPrompt":"latest","sessionId":"first"}"#,
            ],
        );
        write(
            home.path(),
            "-code-my-app",
            "titled",
            &[
                user,
                r#"{"type":"ai-title","aiTitle":"Generated","sessionId":"titled"}"#,
                r#"{"type":"custom-title","customTitle":"Login fix","sessionId":"titled"}"#,
                r#"{"type":"last-prompt","lastPrompt":"latest","sessionId":"titled"}"#,
            ],
        );
        write(
            home.path(),
            "-code-my-app",
            "empty",
            &[r#"{"type":"system","cwd":"/code/my_app"}"#],
        );
        // Started here, then moved into a subdirectory and out of the project.
        write(
            home.path(),
            "-code-my-app",
            "moved",
            &[
                user,
                r#"{"type":"user","cwd":"/code/my_app/src","message":{"content":"next"}}"#,
                r#"{"type":"user","cwd":"/tmp/scratch","message":{"content":"last"}}"#,
            ],
        );
        // Same encoded name, different directory.
        write(
            home.path(),
            "-code-my-app",
            "other",
            &[r#"{"type":"user","cwd":"/code/my.app","message":{"content":"x"}}"#],
        );

        let mut found = sessions(home.path(), Path::new(cwd));
        found.sort_by(|a, b| (&a.title, &a.resume_args).cmp(&(&b.title, &b.resume_args)));
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
                (
                    "fix the login bug",
                    &["--resume".to_owned(), "moved".to_owned()][..]
                ),
            ]
        );
    }
}
