//! OpenCode's background service, asked which conversations are working or
//! waiting for the user. OpenCode cannot report to a session's status file,
//! as its plugins run in that shared service.

use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::Activity;

/// The activity of each conversation in `ids`, asked through the OpenCode
/// executable `program`. Empty when the service cannot be reached.
pub fn activities(program: &Path, ids: &[String]) -> HashMap<String, Activity> {
    let Some(running) = api(program, &["session.active"]).and_then(|output| running(&output))
    else {
        return HashMap::new();
    };
    ids.iter()
        .filter_map(|id| {
            let activity = if !running.contains(id) {
                Activity::Idle
            } else if api(
                program,
                &[
                    "session.permission.list",
                    "--param",
                    &format!("sessionID={id}"),
                ],
            )
            .and_then(|output| has_requests(&output))?
            {
                Activity::Waiting
            } else {
                Activity::Working
            };
            Some((id.clone(), activity))
        })
        .collect()
}

/// Output of `opencode api` with `args`, if it succeeded.
fn api(program: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .arg("api")
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Conversations running a turn, from `session.active`: `{"data":{id:…}}`.
fn running(output: &str) -> Option<Vec<String>> {
    let value: serde_json::Value = serde_json::from_str(output).ok()?;
    Some(value["data"].as_object()?.keys().cloned().collect())
}

/// Whether a request list, `{"data":[…]}`, holds any request.
fn has_requests(output: &str) -> Option<bool> {
    let value: serde_json::Value = serde_json::from_str(output).ok()?;
    Some(!value["data"].as_array()?.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_running_conversations() {
        assert_eq!(
            running(r#"{"data":{"ses_a":{"type":"running"}}}"#),
            Some(vec!["ses_a".to_owned()])
        );
        assert_eq!(running(r#"{"data":{}}"#), Some(Vec::new()));
        assert_eq!(running("error"), None);
    }

    #[test]
    fn reads_pending_requests() {
        assert_eq!(has_requests(r#"{"data":[{"id":"per_1"}]}"#), Some(true));
        assert_eq!(has_requests(r#"{"data":[]}"#), Some(false));
        assert_eq!(has_requests(r#"{"error":"x"}"#), None);
    }

    #[test]
    fn an_unreachable_service_reports_nothing() {
        let ids = ["ses_a".to_owned()];
        assert!(activities(Path::new("/nonexistent/opencode"), &ids).is_empty());
    }
}
