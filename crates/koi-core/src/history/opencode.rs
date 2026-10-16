//! OpenCode keeps its conversations in a database behind its service, so
//! they are listed with `opencode session list --format json`.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use super::{PastSession, one_line};
use crate::Agent;

/// The most recent conversation in `cwd` shown as `shown` in the terminal
/// title, which cuts long titles short and ends them with `…`.
pub(super) fn session_titled(
    agent: &Agent,
    shell: &str,
    cwd: &Path,
    shown: &str,
) -> Option<PastSession> {
    sessions(agent, shell, cwd, TITLE_SEARCH)
        .into_iter()
        .find(|session| shows(shown, &session.title))
}

/// Recent conversations searched for the one a terminal shows.
const TITLE_SEARCH: usize = 20;

/// Whether `shown` is `title`, or `title` cut short with `…`.
fn shows(shown: &str, title: &str) -> bool {
    shown == title
        || shown
            .strip_suffix('…')
            .is_some_and(|start| !start.is_empty() && title.starts_with(start))
}

pub(super) fn sessions(agent: &Agent, shell: &str, cwd: &Path, limit: usize) -> Vec<PastSession> {
    let count = limit.to_string();
    let argv = agent.tool_command(
        shell,
        &["session", "list", "--format", "json", "--max-count", &count],
    );
    let Ok(output) = Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    parse(&String::from_utf8_lossy(&output.stdout), cwd)
}

/// Sessions of `cwd` in the command's output. Shell startup files may print
/// before it, so the list starts at the first line opening a JSON array.
fn parse(output: &str, cwd: &Path) -> Vec<PastSession> {
    let Some(start) = output
        .match_indices('[')
        .map(|(at, _)| at)
        .find(|&at| at == 0 || output[..at].ends_with('\n'))
    else {
        return Vec::new();
    };
    let Ok(serde_json::Value::Array(list)) = serde_json::from_str(&output[start..]) else {
        return Vec::new();
    };
    list.iter()
        .filter(|session| session["directory"].as_str().map(Path::new) == Some(cwd))
        .filter_map(|session| {
            Some(PastSession {
                title: session["title"].as_str().and_then(one_line)?,
                updated: SystemTime::UNIX_EPOCH
                    + Duration::from_millis(session["updated"].as_u64()?),
                resume_args: vec!["--session".to_owned(), session["id"].as_str()?.to_owned()],
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_sessions_of_the_directory_after_shell_noise() {
        let output = r#"Welcome [user]
[
  {"id":"ses_1","title":"Fix login","updated":1700000000000,"directory":"/code/app"},
  {"id":"ses_2","title":"Elsewhere","updated":1700000000000,"directory":"/code/other"},
  {"id":"ses_3","title":"","updated":1700000000000,"directory":"/code/app"}
]
"#;
        let sessions = parse(output, Path::new("/code/app"));
        assert_eq!(
            sessions,
            [PastSession {
                title: "Fix login".into(),
                updated: SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
                resume_args: vec!["--session".into(), "ses_1".into()],
            }]
        );
    }

    #[test]
    fn shortened_titles_match_their_start() {
        assert!(shows("问候", "问候"));
        assert!(shows("Fix the login…", "Fix the login redirect loop"));
        assert!(!shows("Fix the login…", "Refactor login"));
        assert!(!shows("…", "anything"));
        assert!(!shows("问候", "问候一下"));
    }

    #[test]
    fn unreadable_output_lists_nothing() {
        for output in ["", "error: no service", "[not json"] {
            assert!(parse(output, Path::new("/code/app")).is_empty());
        }
    }
}
