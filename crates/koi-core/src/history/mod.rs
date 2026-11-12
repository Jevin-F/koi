//! Past conversations that agents saved on disk, so they can be resumed, and
//! the latest reply in the ones still running.
//!
//! Each agent keeps its own records: Claude Code under `~/.claude/projects`,
//! Codex under `~/.codex/sessions`, pi under `~/.pi/agent/sessions` and
//! OpenCode behind its own command. Only the fields needed for a list are
//! read; records that cannot be read are skipped, since missing history is
//! normal.

mod claude;
mod codex;
mod opencode;
mod pi;
mod reply;

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::Agent;
use crate::integration::Integration;

pub use reply::{Replies, reads_replies};

/// A conversation an agent can pick up again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PastSession {
    pub title: String,
    pub updated: SystemTime,
    /// Arguments that make the agent resume this conversation.
    pub resume_args: Vec<String>,
}

/// The conversations in `cwd` of `agent`, an agent Koi integrates with,
/// most recent first, at most `limit`. Agents that are asked through their
/// own command run it with `shell`.
pub fn past_sessions(
    agent: &Agent,
    shell: &str,
    home: &Path,
    cwd: &Path,
    limit: usize,
) -> Vec<PastSession> {
    let mut sessions = match Integration::for_agent(agent) {
        Some(Integration::Claude) => claude::sessions(home, cwd),
        Some(Integration::Codex) => codex::sessions(home, cwd),
        Some(Integration::Pi) => pi::sessions(home, cwd),
        Some(Integration::OpenCode) => opencode::sessions(agent, shell, cwd, limit),
        None => Vec::new(),
    };
    sessions.sort_by_key(|session| std::cmp::Reverse(session.updated));
    sessions.truncate(limit);
    sessions
}

/// The conversation of `agent` in `cwd` that its terminal title shows as
/// `shown`, for agents that name their terminal after the conversation.
pub fn session_titled(agent: &Agent, shell: &str, cwd: &Path, shown: &str) -> Option<PastSession> {
    match Integration::for_agent(agent)? {
        Integration::OpenCode => opencode::session_titled(agent, shell, cwd, shown),
        Integration::Claude | Integration::Codex | Integration::Pi => None,
    }
}

/// Whether another process of the agent holds the conversation that
/// `resume` reopens, so that the agent started with Koi's arguments could
/// not open it. Codex's shared background server does so; a plain Codex,
/// which goes through that server, still can.
pub fn conversation_held(integration: Integration, home: &Path, resume: &[String]) -> bool {
    match integration {
        Integration::Codex => integration
            .conversation_id(resume)
            .is_some_and(|id| codex::thread_held(home, id)),
        Integration::Claude | Integration::Pi | Integration::OpenCode => false,
    }
}

/// The title the agent itself gave the conversation that `resume` reopens,
/// for agents that title their conversations as they go.
pub fn conversation_title(
    integration: Integration,
    home: &Path,
    resume: &[String],
) -> Option<String> {
    match integration {
        Integration::Codex => integration
            .conversation_id(resume)
            .and_then(|id| codex::thread_name(home, id)),
        Integration::Claude | Integration::Pi | Integration::OpenCode => None,
    }
}

/// The conversation of an agent started by hand at `since`, among
/// `sessions` listed most recent first: the latest one saved since then whose
/// resume arguments are not `held` by another session.
pub fn entered_conversation(
    sessions: Vec<PastSession>,
    since: SystemTime,
    held: &[Vec<String>],
) -> Option<PastSession> {
    sessions
        .into_iter()
        .find(|session| session.updated >= since && !held.contains(&session.resume_args))
}

/// `.jsonl` files directly in `dir`, with their modification times.
fn jsonl_files(dir: &Path) -> Vec<(PathBuf, SystemTime)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .filter_map(|path| {
            let modified = path.metadata().and_then(|meta| meta.modified()).ok()?;
            Some((path, modified))
        })
        .collect()
}

/// Lines of a JSON Lines file; reading stops at the first I/O error.
fn lines(path: &Path) -> impl Iterator<Item = String> {
    File::open(path)
        .into_iter()
        .flat_map(|file| BufReader::new(file).lines().map_while(Result::ok))
}

/// Parses `line` only if it contains `marker`, which is far cheaper than
/// parsing every line of a long conversation.
fn parse_if(line: &str, marker: &str) -> Option<serde_json::Value> {
    line.contains(marker)
        .then(|| serde_json::from_str(line).ok())
        .flatten()
}

/// Text of a message's content: a plain string or a list of text blocks.
fn content_text(content: &serde_json::Value) -> Option<String> {
    match content {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Array(blocks) => blocks
            .iter()
            .find_map(|block| block.get("text").and_then(|text| text.as_str()))
            .map(str::to_owned),
        _ => None,
    }
}

/// A one-line title from free text.
fn one_line(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    Some(line.chars().take(120).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entered_conversations_are_new_and_not_held() {
        let since = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(100);
        let session = |id: &str, at: u64| PastSession {
            title: id.to_owned(),
            updated: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(at),
            resume_args: vec!["resume".to_owned(), id.to_owned()],
        };
        let listed = vec![
            session("held", 300),
            session("mine", 200),
            session("old", 50),
        ];
        let held = [vec!["resume".to_owned(), "held".to_owned()]];
        assert_eq!(
            entered_conversation(listed.clone(), since, &held).map(|found| found.title),
            Some("mine".to_owned())
        );
        assert_eq!(
            entered_conversation(listed[2..].to_vec(), since, &held),
            None
        );
    }

    #[test]
    fn content_text_reads_strings_and_blocks() {
        assert_eq!(
            content_text(&serde_json::json!("hi")).as_deref(),
            Some("hi")
        );
        let blocks = serde_json::json!([{ "type": "image" }, { "type": "text", "text": "fix it" }]);
        assert_eq!(content_text(&blocks).as_deref(), Some("fix it"));
        assert_eq!(content_text(&serde_json::json!(3)), None);
    }

    #[test]
    fn one_line_takes_the_first_non_empty_line() {
        assert_eq!(
            one_line("\n  fix the bug  \nmore").as_deref(),
            Some("fix the bug")
        );
        assert_eq!(one_line(" \n "), None);
        assert_eq!(one_line(&"x".repeat(200)).map(|line| line.len()), Some(120));
    }
}
