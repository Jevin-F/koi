//! The latest reply in a conversation an agent is saving, so that a summary
//! shows what the agent said rather than whatever its screen shows last.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::{claude, codex, content_text, one_line, parse_if};
use crate::integration::Integration;

/// How much of the end of a conversation file is searched for the reply;
/// replies further back than this are not shown.
const TAIL_BYTES: u64 = 1024 * 1024;

/// Whether Koi can read replies from the conversations `integration` saves.
/// OpenCode keeps its conversations behind its own command.
pub fn reads_replies(integration: Integration) -> bool {
    integration != Integration::OpenCode
}

/// Latest replies of conversations, re-read only when their file changes.
#[derive(Default)]
pub struct Replies {
    /// Conversation files, by the arguments that resume them.
    files: HashMap<Vec<String>, PathBuf>,
    /// Latest reply in each file, as of its modification time.
    read: HashMap<PathBuf, (SystemTime, Option<String>)>,
}

impl Replies {
    /// Opening line of the latest reply in the conversation of agent
    /// `integration` in `cwd` that `resume` reopens. `None` until the
    /// conversation has a reply on disk.
    pub fn latest(
        &mut self,
        integration: Integration,
        home: &Path,
        cwd: &Path,
        resume: &[String],
    ) -> Option<String> {
        let path = match self.files.get(resume) {
            Some(path) => path.clone(),
            None => {
                let path = conversation_file(integration, home, cwd, resume)?;
                self.files.insert(resume.to_vec(), path.clone());
                path
            }
        };
        let modified = path.metadata().and_then(|meta| meta.modified()).ok()?;
        if let Some((seen, reply)) = self.read.get(&path)
            && *seen == modified
        {
            return reply.clone();
        }
        let reply = last_reply(integration, &path);
        self.read.insert(path, (modified, reply.clone()));
        reply
    }
}

/// Where the conversation that `resume` reopens is saved.
fn conversation_file(
    integration: Integration,
    home: &Path,
    cwd: &Path,
    resume: &[String],
) -> Option<PathBuf> {
    let id = integration.conversation_id(resume)?;
    match integration {
        Integration::Pi => Some(PathBuf::from(id)),
        Integration::Claude => Some(claude::project_dir(home, cwd).join(format!("{id}.jsonl"))),
        Integration::Codex => codex::rollout_of(home, id),
        Integration::OpenCode => None,
    }
}

/// Opening line of the last reply with text in the conversation file at
/// `path`, written by agent `integration`.
fn last_reply(integration: Integration, path: &Path) -> Option<String> {
    tail(path)?.lines().rev().find_map(|line| {
        let content = match integration {
            Integration::Pi => {
                let entry = parse_if(line, "\"role\":\"assistant\"")?;
                (entry["type"] == "message").then(|| entry["message"]["content"].clone())?
            }
            Integration::Claude => {
                let entry = parse_if(line, "\"type\":\"assistant\"")?;
                // Replies of subagents are not the conversation's own.
                (entry["isSidechain"] != true).then(|| entry["message"]["content"].clone())?
            }
            Integration::Codex => {
                let entry = parse_if(line, "\"role\":\"assistant\"")?;
                (entry["type"] == "response_item" && entry["payload"]["type"] == "message")
                    .then(|| entry["payload"]["content"].clone())?
            }
            Integration::OpenCode => return None,
        };
        content_text(&content).and_then(|text| one_line(&text))
    })
}

/// The last [`TAIL_BYTES`] of the file at `path`, starting at a whole line.
fn tail(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let start = length.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if start == 0 {
        return Some(text);
    }
    // The first line was probably cut short by starting mid-file.
    text.split_once('\n').map(|(_, rest)| rest.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn lines(values: &[serde_json::Value]) -> String {
        values
            .iter()
            .map(|value| format!("{value}\n"))
            .collect::<String>()
    }

    #[test]
    fn reads_the_last_pi_reply_with_text() {
        let temp = TempDir::new();
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            lines(&[
                serde_json::json!({"type": "session", "cwd": "/code"}),
                serde_json::json!({"type": "message", "message": {"role": "assistant", "content": [{"type": "text", "text": "old reply"}]}}),
                serde_json::json!({"type": "message", "message": {"role": "assistant", "content": [{"type": "text", "text": "\n\nFixed the bug.\nDetails follow."}]}}),
                serde_json::json!({"type": "message", "message": {"role": "assistant", "content": [{"type": "toolCall", "name": "bash"}]}}),
                serde_json::json!({"type": "message", "message": {"role": "user", "content": "thanks"}}),
            ]),
        )
        .unwrap();
        let resume = ["--session".to_owned(), path.to_string_lossy().into_owned()];
        let reply =
            Replies::default().latest(Integration::Pi, temp.path(), Path::new("/code"), &resume);
        assert_eq!(reply.as_deref(), Some("Fixed the bug."));
    }

    #[test]
    fn skips_claude_subagent_replies() {
        let home = TempDir::new();
        let cwd = Path::new("/code/app");
        let dir = claude::project_dir(home.path(), cwd);
        std::fs::create_dir_all(&dir).unwrap();
        let id = "0b7c5f3e-1a2b-4c3d-8e9f-001122334455";
        std::fs::write(
            dir.join(format!("{id}.jsonl")),
            lines(&[
                serde_json::json!({"type": "assistant", "message": {"content": [{"type": "text", "text": "Main reply"}]}}),
                serde_json::json!({"type": "assistant", "isSidechain": true, "message": {"content": [{"type": "text", "text": "Subagent"}]}}),
                serde_json::json!({"type": "assistant", "message": {"content": [{"type": "thinking", "thinking": "hmm"}]}}),
            ]),
        )
        .unwrap();
        let resume = ["--resume".to_owned(), id.to_owned()];
        let reply = Replies::default().latest(Integration::Claude, home.path(), cwd, &resume);
        assert_eq!(reply.as_deref(), Some("Main reply"));
    }

    #[test]
    fn finds_the_codex_rollout_by_id() {
        let home = TempDir::new();
        let dir = home.path().join(".codex/sessions/2026/01/02");
        std::fs::create_dir_all(&dir).unwrap();
        let id = "0b7c5f3e-1a2b-4c3d-8e9f-001122334455";
        std::fs::write(
            dir.join(format!("rollout-2026-01-02T10-00-00-{id}.jsonl")),
            lines(&[
                serde_json::json!({"type": "response_item", "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Done"}]}}),
                serde_json::json!({"type": "event_msg", "payload": {"type": "task_complete"}}),
            ]),
        )
        .unwrap();
        let resume = ["resume".to_owned(), id.to_owned()];
        let reply =
            Replies::default().latest(Integration::Codex, home.path(), Path::new("/code"), &resume);
        assert_eq!(reply.as_deref(), Some("Done"));
    }

    #[test]
    fn rereads_a_conversation_only_after_it_changes() {
        let temp = TempDir::new();
        let path = temp.path().join("session.jsonl");
        let reply = |text: &str| {
            lines(&[
                serde_json::json!({"type": "message", "message": {"role": "assistant", "content": [{"type": "text", "text": text}]}}),
            ])
        };
        std::fs::write(&path, reply("first")).unwrap();
        let resume = ["--session".to_owned(), path.to_string_lossy().into_owned()];
        let mut replies = Replies::default();
        let latest = |replies: &mut Replies| {
            replies.latest(Integration::Pi, temp.path(), temp.path(), &resume)
        };
        assert_eq!(latest(&mut replies).as_deref(), Some("first"));
        let modified = path.metadata().unwrap().modified().unwrap();
        std::fs::write(&path, reply("second")).unwrap();
        // An unchanged modification time keeps the earlier reading.
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        assert_eq!(latest(&mut replies).as_deref(), Some("first"));
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified + std::time::Duration::from_secs(1))
            .unwrap();
        assert_eq!(latest(&mut replies).as_deref(), Some("second"));
    }

    #[test]
    fn has_no_reply_without_a_conversation() {
        let temp = TempDir::new();
        let missing = ["--session".to_owned(), "/nowhere/a.jsonl".to_owned()];
        let mut replies = Replies::default();
        assert_eq!(
            replies.latest(Integration::Pi, temp.path(), temp.path(), &missing),
            None
        );
        assert_eq!(
            replies.latest(Integration::Pi, temp.path(), temp.path(), &[]),
            None
        );
        assert!(!reads_replies(Integration::OpenCode));
        assert!(reads_replies(Integration::Pi));
    }

    #[test]
    fn a_long_file_is_read_from_a_whole_line() {
        let temp = TempDir::new();
        let path = temp.path().join("long.jsonl");
        let filler = format!("{}\n", "x".repeat(TAIL_BYTES as usize));
        std::fs::write(&path, format!("{filler}last line\n")).unwrap();
        assert_eq!(tail(&path).as_deref(), Some("last line\n"));
    }
}
