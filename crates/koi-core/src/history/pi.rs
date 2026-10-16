//! pi: `~/.pi/agent/sessions/--<cwd without its leading '/', with '/', '\'
//! and ':' replaced by '-'>--/<timestamp>_<id>.jsonl`.

use std::path::Path;

use super::{PastSession, content_text, jsonl_files, lines, one_line, parse_if};

pub(super) fn sessions(home: &Path, cwd: &Path) -> Vec<PastSession> {
    let path = cwd.to_string_lossy();
    let encoded: String = path
        .strip_prefix('/')
        .unwrap_or(&path)
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':') {
                '-'
            } else {
                c
            }
        })
        .collect();
    jsonl_files(
        &home
            .join(".pi/agent/sessions")
            .join(format!("--{encoded}--")),
    )
    .into_iter()
    .filter_map(|(path, updated)| {
        let title = title(&path, cwd)?;
        Some(PastSession {
            title,
            updated,
            resume_args: vec!["--session".to_owned(), path.to_string_lossy().into_owned()],
        })
    })
    .collect()
}

/// The session's name, else its first prompt. `None` if the file belongs to
/// another directory or has no prompt.
fn title(path: &Path, cwd: &Path) -> Option<String> {
    let mut lines = lines(path);
    let header = parse_if(&lines.next()?, "\"session\"")?;
    if header["cwd"].as_str().map(Path::new) != Some(cwd) {
        return None;
    }
    let (mut name, mut first_prompt) = (None, None);
    for line in lines {
        if let Some(entry) = parse_if(&line, "\"session_info\"") {
            name = entry["name"].as_str().and_then(one_line).or(name);
        } else if first_prompt.is_none()
            && let Some(entry) = parse_if(&line, "\"role\":\"user\"")
        {
            first_prompt =
                content_text(&entry["message"]["content"]).and_then(|text| one_line(&text));
        }
    }
    name.or(first_prompt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn lists_named_and_unnamed_sessions() {
        let home = TempDir::new();
        let dir = home.path().join(".pi/agent/sessions/--code-app--");
        std::fs::create_dir_all(&dir).unwrap();
        let header = r#"{"type":"session","version":3,"id":"s1","cwd":"/code/app"}"#;
        let user = r#"{"type":"message","message":{"role":"user","content":[{"type":"text","text":"write docs"}]}}"#;
        let named = r#"{"type":"session_info","name":"Docs pass"}"#;
        std::fs::write(dir.join("1_a.jsonl"), format!("{header}\n{user}\n")).unwrap();
        std::fs::write(
            dir.join("2_b.jsonl"),
            format!("{header}\n{user}\n{named}\n"),
        )
        .unwrap();
        std::fs::write(dir.join("3_c.jsonl"), format!("{header}\n")).unwrap();

        let mut found = sessions(home.path(), Path::new("/code/app"));
        found.sort_by(|a, b| a.title.cmp(&b.title));
        let titles: Vec<&str> = found.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["Docs pass", "write docs"]);
        assert_eq!(
            found[0].resume_args,
            [
                "--session".to_owned(),
                dir.join("2_b.jsonl").to_string_lossy().into_owned()
            ]
        );
    }
}
