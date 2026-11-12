//! Codex: `~/.codex/sessions/<year>/<month>/<day>/rollout-*.jsonl`, whose
//! first line describes the session, plus titles in
//! `~/.codex/session_index.jsonl`.

use std::collections::HashMap;
use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::{PastSession, content_text, jsonl_files, lines, one_line, parse_if};

pub(super) fn sessions(home: &Path, cwd: &Path) -> Vec<PastSession> {
    let names = thread_names(&home.join(".codex/session_index.jsonl"));
    rollouts(&home.join(".codex/sessions"))
        .into_iter()
        .filter_map(|(path, updated)| {
            let meta = parse_if(&lines(&path).next()?, "\"session_meta\"")?;
            let payload = &meta["payload"];
            if payload["cwd"].as_str().map(Path::new) != Some(cwd) {
                return None;
            }
            let id = payload["id"].as_str()?.to_owned();
            let title = names.get(&id).cloned().or_else(|| first_prompt(&path))?;
            Some(PastSession {
                title,
                updated,
                resume_args: vec!["resume".to_owned(), id],
            })
        })
        .collect()
}

/// Whether another Codex process holds conversation `id` open for writing,
/// with a lock in `~/.codex/thread-writer-locks`. Codex's shared background
/// server keeps the conversations started through it open after their
/// terminal exits.
pub(super) fn thread_held(home: &Path, id: &str) -> bool {
    let lock = home.join(format!(".codex/thread-writer-locks/{id}.lock"));
    File::open(lock)
        .is_ok_and(|file| matches!(file.try_lock_shared(), Err(TryLockError::WouldBlock)))
}

/// The latest title of conversation `id`, which Codex gives it soon after
/// the first prompt and changes on `/rename`.
pub(super) fn thread_name(home: &Path, id: &str) -> Option<String> {
    thread_names(&home.join(".codex/session_index.jsonl")).remove(id)
}

/// Session titles by id; later lines rename earlier ones.
fn thread_names(index: &Path) -> HashMap<String, String> {
    lines(index)
        .filter_map(|line| {
            let entry: serde_json::Value = serde_json::from_str(&line).ok()?;
            let name = entry["thread_name"].as_str().and_then(one_line)?;
            Some((entry["id"].as_str()?.to_owned(), name))
        })
        .collect()
}

/// The rollout file of conversation `id`, named `rollout-<time>-<id>.jsonl`.
pub(super) fn rollout_of(home: &Path, id: &str) -> Option<PathBuf> {
    let suffix = format!("-{id}.jsonl");
    rollouts(&home.join(".codex/sessions"))
        .into_iter()
        .map(|(path, _)| path)
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(&suffix))
        })
}

/// Rollout files in the date directories below `root`.
fn rollouts(root: &Path) -> Vec<(PathBuf, SystemTime)> {
    let subdirs = |dir: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect()
    };
    subdirs(root)
        .iter()
        .flat_map(|year| subdirs(year))
        .flat_map(|month| subdirs(&month))
        .flat_map(|day| jsonl_files(&day))
        .collect()
}

/// The first prompt typed by the user, skipping context Codex injects as
/// user messages (wrapped in tags).
fn first_prompt(path: &Path) -> Option<String> {
    lines(path).find_map(|line| {
        let entry = parse_if(&line, "\"role\":\"user\"")?;
        let text = content_text(&entry["payload"]["content"])?;
        (!text.trim_start().starts_with('<'))
            .then(|| one_line(&text))
            .flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn rollout(home: &Path, name: &str, id: &str, cwd: &str, prompt: &str) {
        let dir = home.join(".codex/sessions/2026/01/02");
        std::fs::create_dir_all(&dir).unwrap();
        let meta = serde_json::json!({"type": "session_meta", "payload": {"id": id, "cwd": cwd}});
        let context = serde_json::json!({"type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "<environment_context>…"}]}});
        let user = serde_json::json!({"type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": prompt}]}});
        std::fs::write(dir.join(name), format!("{meta}\n{context}\n{user}\n")).unwrap();
    }

    #[test]
    fn tells_conversations_held_by_another_process() {
        let home = TempDir::new();
        assert!(!thread_held(home.path(), "a"), "no lock file");
        let dir = home.path().join(".codex/thread-writer-locks");
        std::fs::create_dir_all(&dir).unwrap();
        let writer = File::create(dir.join("a.lock")).unwrap();
        assert!(!thread_held(home.path(), "a"), "released");
        writer.lock().unwrap();
        assert!(thread_held(home.path(), "a"));
        writer.unlock().unwrap();
        assert!(!thread_held(home.path(), "a"));
    }

    #[test]
    fn lists_sessions_of_the_directory_with_index_titles() {
        let home = TempDir::new();
        rollout(
            home.path(),
            "rollout-a.jsonl",
            "a",
            "/code/app",
            "add tests",
        );
        rollout(home.path(), "rollout-b.jsonl", "b", "/code/app", "unused");
        rollout(
            home.path(),
            "rollout-c.jsonl",
            "c",
            "/code/other",
            "elsewhere",
        );
        std::fs::write(
            home.path().join(".codex/session_index.jsonl"),
            "{\"id\":\"b\",\"thread_name\":\"Old name\"}\n{\"id\":\"b\",\"thread_name\":\"Refactor\"}\n",
        )
        .unwrap();

        let mut found = sessions(home.path(), Path::new("/code/app"));
        found.sort_by(|a, b| a.title.cmp(&b.title));
        let summary: Vec<(&str, &str)> = found
            .iter()
            .map(|s| (s.title.as_str(), s.resume_args[1].as_str()))
            .collect();
        assert_eq!(summary, [("Refactor", "b"), ("add tests", "a")]);
        assert_eq!(found[0].resume_args[0], "resume");
        assert_eq!(thread_name(home.path(), "b").as_deref(), Some("Refactor"));
        assert_eq!(thread_name(home.path(), "a"), None);
    }
}
