//! Agents: the programs Koi starts in a terminal.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::integration::STATUS_ENV;

/// A program that can run in a session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    /// Stable identifier, stored with sessions.
    pub id: String,
    /// Name shown in the interface.
    pub name: String,
    /// Program and arguments. Empty means the user's interactive shell.
    #[serde(default)]
    pub command: Vec<String>,
}

impl Agent {
    fn new(id: &str, name: &str, command: &[&str]) -> Self {
        Self {
            id: id.to_owned(),
            name: name.to_owned(),
            command: command.iter().map(|part| (*part).to_owned()).collect(),
        }
    }

    /// Program and arguments that start this agent through `shell`.
    ///
    /// Agents run inside an interactive login shell so they see the same
    /// `PATH` and environment as in a regular terminal, even when Koi itself
    /// was launched from Finder.
    /// When the agent exits, an interactive shell takes over in the same directory.
    ///
    /// `extra` arguments are appended to the agent's own; they are ignored
    /// for the plain shell.
    pub fn launch_command(&self, shell: &str, extra: &[String]) -> Vec<String> {
        let mut command = shell_command(shell);
        if !self.command.is_empty() {
            let line = self
                .command
                .iter()
                .chain(extra)
                .map(|part| shell_quote(part))
                .collect::<Vec<_>>();
            let script = format!(
                "if {}; then koi_agent_exit=0; else koi_agent_exit=$?; fi; \
                 if [ -n \"${{{STATUS_ENV}:-}}\" ]; then \
                 printf 'shell %s\\n' \"$koi_agent_exit\" > \"${STATUS_ENV}\"; fi; \
                 exec {} -i",
                line.join(" "),
                shell_quote(shell),
            );
            command.extend([
                "-i".to_owned(),
                "-c".to_owned(),
                format!("exec /bin/sh -c {}", shell_quote(&script)),
            ]);
        }
        command
    }

    /// Program and arguments that run the agent's program once with `args`,
    /// through the same shell setup as [`Self::launch_command`].
    pub fn tool_command(&self, shell: &str, args: &[&str]) -> Vec<String> {
        let line = self
            .command
            .iter()
            .map(String::as_str)
            .chain(args.iter().copied())
            .map(shell_quote)
            .collect::<Vec<_>>();
        vec![
            shell.to_owned(),
            "-l".to_owned(),
            "-i".to_owned(),
            "-c".to_owned(),
            format!("exec {}", line.join(" ")),
        ]
    }

    /// File name of the agent's program, such as `pi` for `/opt/bin/pi`.
    pub fn program_name(&self) -> Option<&str> {
        let program = self.command.first()?;
        program.rsplit('/').next()
    }

    /// Full path of the agent's program, found on the `PATH` of the user's
    /// shell `shell`, so it can be run often without starting a shell.
    pub fn locate(&self, shell: &str) -> Option<PathBuf> {
        let program = self.command.first()?;
        let output = Command::new(shell)
            .args(["-l", "-i", "-c", "printf '\\n%s' \"$PATH\""])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        // The value comes last, after anything the shell's startup printed.
        let path = String::from_utf8_lossy(&output.stdout);
        find_program(path.lines().last()?, program)
    }
}

/// `program` itself if it is a path, else the first executable file named
/// `program` in the directories of `path`, a `PATH` value.
fn find_program(path: &str, program: &str) -> Option<PathBuf> {
    let is_executable = |file: &Path| {
        file.metadata()
            .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    };
    if program.contains('/') {
        let file = PathBuf::from(program);
        return (file.is_absolute() && is_executable(&file)).then_some(file);
    }
    path.split(':')
        .filter(|dir| dir.starts_with('/'))
        .map(|dir| Path::new(dir).join(program))
        .find(|file| is_executable(file))
}

/// The agents Koi knows without any configuration.
pub(crate) fn builtin_agents() -> Vec<Agent> {
    vec![
        Agent::new("claude", "Claude Code", &["claude"]),
        Agent::new("codex", "Codex", &["codex"]),
        Agent::new("opencode", "OpenCode", &["opencode"]),
        Agent::new("pi", "Pi", &["pi"]),
        Agent::new("shell", "Shell", &[]),
    ]
}

/// Program and arguments of the user's interactive login `shell` alone.
pub fn shell_command(shell: &str) -> Vec<String> {
    vec![shell.to_owned(), "-l".to_owned()]
}

/// Quotes `word` for a POSIX shell.
fn shell_quote(word: &str) -> String {
    let safe = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:,+@%".contains(c));
    if safe {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting() {
        assert_eq!(shell_quote("claude"), "claude");
        assert_eq!(shell_quote("--model=opus"), "--model=opus");
        assert_eq!(shell_quote("two words"), "'two words'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn agents_run_through_an_interactive_login_shell() {
        let agent = Agent::new("x", "X", &["claude", "--resume", "a b"]);
        let command = agent.launch_command(
            "/bin/zsh",
            &["--settings".to_owned(), "{\"a\": 1}".to_owned()],
        );
        assert_eq!(&command[..4], ["/bin/zsh", "-l", "-i", "-c"]);
        assert!(command[4].starts_with("exec /bin/sh -c "));
        assert!(command[4].contains("claude --resume"));
        assert!(command[4].contains("exec /bin/zsh -i"));
    }

    #[test]
    fn tool_commands_run_once_through_the_login_shell() {
        let agent = Agent::new("x", "X", &["opencode"]);
        assert_eq!(
            agent.tool_command("/bin/zsh", &["session", "list", "a b"]),
            [
                "/bin/zsh",
                "-l",
                "-i",
                "-c",
                "exec opencode session list 'a b'"
            ]
        );
    }

    #[test]
    fn program_names_drop_the_directory() {
        assert_eq!(
            Agent::new("x", "X", &["/opt/bin/pi", "-c"]).program_name(),
            Some("pi")
        );
        assert_eq!(
            Agent::new("x", "X", &["codex"]).program_name(),
            Some("codex")
        );
        assert_eq!(Agent::new("x", "X", &[]).program_name(), None);
    }

    #[test]
    fn programs_are_found_on_the_path() {
        let temp = crate::test_support::TempDir::new();
        let (first, second) = (temp.path().join("a"), temp.path().join("b"));
        for dir in [&first, &second] {
            std::fs::create_dir(dir).unwrap();
        }
        std::fs::write(first.join("tool"), "").unwrap();
        std::fs::write(second.join("tool"), "").unwrap();
        let executable = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(second.join("tool"), executable).unwrap();
        let path = format!("relative:{}:{}", first.display(), second.display());
        assert_eq!(find_program(&path, "tool"), Some(second.join("tool")));
        assert_eq!(find_program(&path, "missing"), None);
        assert_eq!(find_program("", "/bin/sh"), Some(PathBuf::from("/bin/sh")));
    }

    #[test]
    fn programs_are_located_through_the_shell() {
        let agent = Agent::new("x", "X", &["sh"]);
        assert!(
            agent
                .locate("/bin/sh")
                .is_some_and(|path| path.ends_with("sh"))
        );
        assert_eq!(Agent::new("x", "X", &[]).locate("/bin/sh"), None);
    }

    #[test]
    fn shell_agent_is_a_login_shell() {
        let shell = builtin_agents()
            .into_iter()
            .find(|agent| agent.id == "shell")
            .unwrap();
        assert_eq!(shell.launch_command("/bin/zsh", &[]), ["/bin/zsh", "-l"]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn agent_exit_returns_to_a_usable_shell_in_the_working_directory() {
        use std::io::Write;
        use std::process::{Command, Stdio};

        use crate::integration::SessionReport;
        use crate::test_support::TempDir;

        for (program, code) in [
            (vec!["/bin/sh", "-c", "exit 0"], 0),
            (vec!["/bin/sh", "-c", "exit 7"], 7),
            (vec!["/bin/sh", "-c", "kill -INT $$"], 130),
            (vec!["/nonexistent/koi-agent"], 127),
        ] {
            let temp = TempDir::new();
            let status = temp.path().join("status");
            let agent = Agent::new("test", "Test", &program);
            let command = agent.launch_command("/bin/zsh", &[]);
            let mut child = Command::new(&command[0])
                .args(&command[1..])
                .env("ZDOTDIR", temp.path())
                .env(STATUS_ENV, &status)
                .current_dir(temp.path())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(b"printf 'shell-ready:'; pwd; exit\n")
                .unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let expected = format!("shell-ready:{}", temp.path().display());
            assert!(String::from_utf8_lossy(&output.stdout).contains(&expected));
            assert_eq!(
                SessionReport::parse(&std::fs::read_to_string(status).unwrap()),
                Some(SessionReport::Shell(code))
            );
        }
    }
}
