//! Makes agents report their activity to Koi.
//!
//! Koi starts each agent with the path of a status file in [`STATUS_ENV`]
//! and adds arguments that make the agent write an [`Activity`] word there.
//! Nothing in the user's own agent configuration is changed.

use serde_json::json;
use std::io::{self, Read};
use std::path::Path;

use crate::{Activity, Agent, AppPaths};

/// Environment variable holding the session's status file.
pub const STATUS_ENV: &str = "KOI_STATUS_FILE";

/// Environment variable holding a session's first submitted prompt excerpt.
pub const TITLE_ENV: &str = "KOI_TITLE_FILE";

/// Environment variable holding the path for an agent's resumable session.
pub const RESUME_ENV: &str = "KOI_RESUME_FILE";

/// Environment variable making Pi report its state with OSC 7501 without
/// asking the terminal first. Set for every session, it also reaches a Pi
/// started by hand in the session's shell.
pub const PI_PROGRAM_STATUS_ENV: &str = "PI_PROGRAM_STATUS";

/// Environment variable with Koi's executable path for agent hooks.
pub const EXECUTABLE_ENV: &str = "KOI_EXECUTABLE";

/// Argument making Koi record a Claude Code prompt read from standard input.
pub const RECORD_PROMPT_ARG: &str = "--record-prompt";

/// Argument making Koi record a finished Codex turn, given as the next argument.
pub const RECORD_TURN_ARG: &str = "--record-turn";

/// Reads Claude Code's `UserPromptSubmit` hook input and records a bounded
/// prompt excerpt and the conversation's id.
pub fn record_claude_prompt() -> io::Result<()> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    if let Some(prompt) = payload_field(&input, "prompt") {
        write_env_file(TITLE_ENV, &prompt.chars().take(512).collect::<String>())?;
    }
    if let Some(id) = payload_field(&input, "session_id") {
        write_env_file(RESUME_ENV, &id)?;
    }
    Ok(())
}

/// Records the conversation id from the payload Codex hands its `notify`
/// program when a turn completes.
pub fn record_codex_turn(payload: &str) -> io::Result<()> {
    match payload_field(payload, "thread-id") {
        Some(id) => write_env_file(RESUME_ENV, &id),
        None => Ok(()),
    }
}

/// A string field of a JSON object.
fn payload_field(payload: &str, key: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(payload).ok()?;
    value.get(key)?.as_str().map(str::to_owned)
}

/// Writes `contents` to the file named by environment variable `name`, if set.
fn write_env_file(name: &str, contents: &str) -> io::Result<()> {
    match std::env::var_os(name) {
        Some(path) => std::fs::write(path, contents),
        None => Ok(()),
    }
}

/// Source of the pi extension that reports activity.
pub const PI_EXTENSION: &str = include_str!("../integrations/pi-status.ts");

/// A status file update from an agent or its launching shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionReport {
    /// Activity reported while the agent is running.
    Activity(Activity),
    /// The agent exited with this code and control is returning to the shell.
    Shell(u32),
}

impl SessionReport {
    /// Parses complete reports, ignoring partial writes and unknown values.
    pub fn parse(contents: &str) -> Option<Self> {
        let contents = contents.trim();
        if let Some(code) = contents.strip_prefix("shell ") {
            code.parse().ok().map(Self::Shell)
        } else {
            Activity::parse(contents).map(Self::Activity)
        }
    }
}

/// How an agent reports its activity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Integration {
    /// Claude Code hooks, loaded with `--settings`.
    Claude,
    /// Codex's `notify` program, run when a turn completes. Reports only
    /// [`Activity::Idle`]; Codex hooks would need the user to trust them.
    Codex,
    /// A pi extension, loaded with `--extension`.
    Pi,
    /// OpenCode, whose plugins run in a shared background service out of
    /// reach of a session's status file; that service is asked instead.
    OpenCode,
}

impl Integration {
    /// The integration for `agent`, recognised by its program name, so
    /// configured variants such as `claude --model opus` are covered too.
    pub fn for_agent(agent: &Agent) -> Option<Self> {
        let program = agent.command.first()?;
        match program.rsplit('/').next()? {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "pi" => Some(Self::Pi),
            "opencode" => Some(Self::OpenCode),
            _ => None,
        }
    }

    /// Whether the agent writes its activity to the status file. OpenCode's
    /// is asked from its service instead.
    pub fn reports_activity(self) -> bool {
        self != Self::OpenCode
    }

    /// Arguments that reopen the conversation the agent reported in its
    /// resume file: Pi writes its session file, Claude Code and Codex an id.
    /// OpenCode is looked up instead, and its id is passed the same way.
    pub fn resume_args(self, reported: &str) -> Option<Vec<String>> {
        let reported = reported.trim();
        let (flag, valid) = match self {
            Self::Pi => (
                "--session",
                Path::new(reported).is_absolute() && reported.ends_with(".jsonl"),
            ),
            Self::Claude => ("--resume", is_session_id(reported)),
            Self::Codex => ("resume", is_session_id(reported)),
            Self::OpenCode => ("--session", is_session_id(reported)),
        };
        valid.then(|| vec![flag.to_owned(), reported.to_owned()])
    }

    /// The conversation that `resume`, made by [`Self::resume_args`], reopens.
    pub fn conversation_id(self, resume: &[String]) -> Option<&str> {
        match resume {
            [_, id] => self
                .resume_args(id)
                .filter(|args| args == resume)
                .map(|_| id.as_str()),
            _ => None,
        }
    }

    /// The conversation title in the agent's terminal title, for agents that
    /// name their terminal after it. OpenCode shows `OC | <title>`, with
    /// long titles cut short and ended with `…`.
    pub fn session_title(self, terminal_title: &str) -> Option<&str> {
        match self {
            Self::OpenCode => terminal_title.strip_prefix("OC | "),
            _ => None,
        }
    }

    /// Arguments to add to the agent's command line.
    pub fn args(self, paths: &AppPaths) -> Vec<String> {
        match self {
            Self::Claude => vec!["--settings".to_owned(), claude_settings()],
            // Codex appends the turn's payload, which becomes `$1`.
            Self::Codex => vec![
                "-c".to_owned(),
                format!(
                    "notify=[\"sh\", \"-c\", {}, \"sh\"]",
                    toml_string(&format!(
                        "{}; {}",
                        report_command(Activity::Idle),
                        run_koi(&format!("{RECORD_TURN_ARG} \"$1\""))
                    ))
                ),
            ],
            Self::Pi => vec![
                "--extension".to_owned(),
                paths.pi_extension_file().to_string_lossy().into_owned(),
            ],
            Self::OpenCode => Vec::new(),
        }
    }
}

/// A conversation id as agents print it: letters, digits, `-` and `_`.
fn is_session_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 128
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Shell command running Koi with `args`, when the agent was started by Koi.
fn run_koi(args: &str) -> String {
    format!("if [ -n \"${{{EXECUTABLE_ENV}:-}}\" ]; then \"${EXECUTABLE_ENV}\" {args}; fi")
}

/// Shell command that writes `activity` to the status file.
fn report_command(activity: Activity) -> String {
    format!("printf {} > \"${STATUS_ENV}\"", activity.as_str())
}

/// Claude Code settings adding hooks: a prompt or finished tool call means
/// working, a permission prompt means waiting, and stopping means idle.
fn claude_settings() -> String {
    let hook = |activity: Activity| json!([{ "hooks": [{ "type": "command", "command": report_command(activity) }] }]);
    json!({
        "hooks": {
            "UserPromptSubmit": [{ "hooks": [{
                "type": "command",
                "command": format!(
                    "{}; {}",
                    report_command(Activity::Working),
                    run_koi(RECORD_PROMPT_ARG)
                )
            }] }],
            "PostToolUse": hook(Activity::Working),
            "Notification": [{
                "matcher": "permission_prompt",
                "hooks": [{ "type": "command", "command": report_command(Activity::Waiting) }],
            }],
            "Stop": hook(Activity::Idle),
        }
    })
    .to_string()
}

/// A TOML basic string.
fn toml_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parses_agent_and_shell_reports() {
        assert_eq!(
            SessionReport::parse("working\n"),
            Some(SessionReport::Activity(Activity::Working))
        );
        assert_eq!(
            SessionReport::parse("shell 0\n"),
            Some(SessionReport::Shell(0))
        );
        assert_eq!(
            SessionReport::parse("shell 130"),
            Some(SessionReport::Shell(130))
        );
        for invalid in [
            "",
            "shell",
            "shell ",
            "shell -1",
            "shell x",
            "shell 1 extra",
        ] {
            assert_eq!(SessionReport::parse(invalid), None);
        }
    }

    fn agent(command: &[&str]) -> Agent {
        Agent {
            id: "x".into(),
            name: "X".into(),
            command: command.iter().map(|part| (*part).to_owned()).collect(),
        }
    }

    #[test]
    fn recognises_agents_by_program_name() {
        assert_eq!(
            Integration::for_agent(&agent(&["claude", "--model", "opus"])),
            Some(Integration::Claude)
        );
        assert_eq!(
            Integration::for_agent(&agent(&["/opt/bin/codex"])),
            Some(Integration::Codex)
        );
        assert_eq!(
            Integration::for_agent(&agent(&["pi"])),
            Some(Integration::Pi)
        );
        assert_eq!(
            Integration::for_agent(&agent(&["opencode"])),
            Some(Integration::OpenCode)
        );
        assert!(!Integration::OpenCode.reports_activity());
        assert!(Integration::Pi.reports_activity());
        assert_eq!(Integration::for_agent(&agent(&["aider"])), None);
        assert_eq!(Integration::for_agent(&agent(&[])), None);
    }

    #[test]
    fn claude_hooks_write_the_status_file() {
        let args = Integration::Claude.args(&AppPaths::in_home(Path::new("/h")));
        assert_eq!(args[0], "--settings");
        let settings: serde_json::Value = serde_json::from_str(&args[1]).unwrap();
        let stop = &settings["hooks"]["Stop"][0]["hooks"][0]["command"];
        assert_eq!(stop, "printf idle > \"$KOI_STATUS_FILE\"");
        let submit = &settings["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"];
        assert!(
            submit
                .as_str()
                .unwrap()
                .contains("$KOI_EXECUTABLE\" --record-prompt")
        );
        assert_eq!(
            settings["hooks"]["Notification"][0]["matcher"],
            "permission_prompt"
        );
    }

    #[test]
    fn hook_payload_fields_are_parsed_as_json() {
        let payload = r#"{"prompt":"Review PR 385","session_id":"abc-1","n":42}"#;
        assert_eq!(
            payload_field(payload, "prompt"),
            Some("Review PR 385".into())
        );
        assert_eq!(payload_field(payload, "session_id"), Some("abc-1".into()));
        assert_eq!(payload_field(payload, "n"), None);
        assert_eq!(payload_field("not json", "prompt"), None);
    }

    #[test]
    fn reported_conversations_become_resume_arguments() {
        let id = "01a13fc1-cb1e-7102-84cc-4f0125fde8b5";
        assert_eq!(
            Integration::Claude.resume_args(id),
            Some(vec!["--resume".into(), id.into()])
        );
        assert_eq!(
            Integration::Codex.resume_args(&format!("{id}\n")),
            Some(vec!["resume".into(), id.into()])
        );
        assert_eq!(
            Integration::OpenCode.resume_args("ses_ed0b3b96affe"),
            Some(vec!["--session".into(), "ses_ed0b3b96affe".into()])
        );
        assert_eq!(
            Integration::Pi.resume_args("/h/.pi/agent/sessions/a.jsonl"),
            Some(vec![
                "--session".into(),
                "/h/.pi/agent/sessions/a.jsonl".into()
            ])
        );
        assert_eq!(Integration::Pi.resume_args("relative.jsonl"), None);
        assert_eq!(Integration::Claude.resume_args(""), None);
        assert_eq!(Integration::Claude.resume_args("a b; rm -rf"), None);
    }

    #[test]
    fn resume_arguments_name_their_conversation() {
        let resume = Integration::OpenCode.resume_args("ses_a").unwrap();
        assert_eq!(
            Integration::OpenCode.conversation_id(&resume),
            Some("ses_a")
        );
        assert_eq!(
            Integration::OpenCode.conversation_id(&["--resume".into(), "ses_a".into()]),
            None
        );
        assert_eq!(Integration::OpenCode.conversation_id(&[]), None);
    }

    #[test]
    fn opencode_terminal_titles_name_the_conversation() {
        assert_eq!(
            Integration::OpenCode.session_title("OC | 问候"),
            Some("问候")
        );
        assert_eq!(Integration::OpenCode.session_title("OpenCode"), None);
        assert_eq!(Integration::Claude.session_title("OC | x"), None);
    }

    #[test]
    fn codex_notify_is_valid_toml() {
        let args = Integration::Codex.args(&AppPaths::in_home(Path::new("/h")));
        let parsed: toml::Table = toml::from_str(&args[1]).unwrap();
        let notify: Vec<&str> = parsed["notify"]
            .as_array()
            .unwrap()
            .iter()
            .map(|part| part.as_str().unwrap())
            .collect();
        assert_eq!(notify.len(), 4);
        assert_eq!(notify[3], "sh");
        assert!(notify[2].starts_with("printf idle > \"$KOI_STATUS_FILE\"; "));
        assert!(notify[2].contains("\"$KOI_EXECUTABLE\" --record-turn \"$1\""));
    }

    #[test]
    fn pi_loads_the_extension_file() {
        let paths = AppPaths::in_home(Path::new("/h"));
        let args = Integration::Pi.args(&paths);
        assert_eq!(
            args,
            [
                "--extension".to_owned(),
                paths.pi_extension_file().to_string_lossy().into_owned()
            ]
        );
    }
}
