//! Running processes, read with `ps`: what they use and which ones run
//! below a terminal.

use std::collections::HashMap;
use std::io;
use std::process::Command;

use crate::Agent;

/// What a set of processes uses.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Usage {
    /// Resident memory, in bytes.
    pub memory: u64,
    /// Share of one CPU core, in percent; above 100 when several cores are busy.
    pub cpu: f32,
}

impl std::ops::Add for Usage {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            memory: self.memory + other.memory,
            cpu: self.cpu + other.cpu,
        }
    }
}

struct Process {
    pid: u32,
    parent: u32,
    usage: Usage,
    command: String,
}

/// A snapshot of the running processes.
pub struct ProcessTable {
    processes: Vec<Process>,
    /// Indices into `processes` by parent pid.
    children: HashMap<u32, Vec<usize>>,
}

impl ProcessTable {
    /// Takes a snapshot of all processes.
    pub fn read() -> io::Result<Self> {
        let output = Command::new("ps")
            .args(["-A", "-ww", "-o", "pid=,ppid=,rss=,%cpu=,command="])
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }
        Ok(Self::parse(&String::from_utf8_lossy(&output.stdout)))
    }

    /// Parses lines of `pid ppid rss-in-KiB cpu-percent command`, skipping malformed ones.
    pub(crate) fn parse(output: &str) -> Self {
        let processes: Vec<Process> = output
            .lines()
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                let pid = fields.next()?.parse().ok()?;
                let parent = fields.next()?.parse().ok()?;
                let rss_kib: u64 = fields.next()?.parse().ok()?;
                let cpu = fields.next()?.parse().ok()?;
                let command = fields.collect::<Vec<_>>().join(" ");
                Some(Process {
                    pid,
                    parent,
                    usage: Usage {
                        memory: rss_kib * 1024,
                        cpu,
                    },
                    command,
                })
            })
            .collect();
        let mut children: HashMap<u32, Vec<usize>> = HashMap::new();
        for (ix, process) in processes.iter().enumerate() {
            children.entry(process.parent).or_default().push(ix);
        }
        Self {
            processes,
            children,
        }
    }

    /// What process `pid` alone uses; `None` if it is not running.
    pub fn process(&self, pid: u32) -> Option<Usage> {
        self.processes
            .iter()
            .find(|process| process.pid == pid)
            .map(|process| process.usage)
    }

    /// What process `pid` and everything it started use; `None` if it is
    /// not running.
    pub fn tree(&self, pid: u32) -> Option<Usage> {
        let own = self.process(pid)?;
        Some(
            self.descendants(pid)
                .iter()
                .fold(own, |total, child| total + child.usage),
        )
    }

    /// The first of `agents` whose program runs in a process below `pid`.
    pub fn agent_below<'a>(&self, pid: u32, agents: &'a [Agent]) -> Option<&'a Agent> {
        self.descendants(pid).into_iter().find_map(|child| {
            agents.iter().find(|agent| {
                agent
                    .program_name()
                    .is_some_and(|program| runs(&child.command, program))
            })
        })
    }

    /// Process `pid`, if running, and every process below it, each with its
    /// command line.
    pub(crate) fn family(&self, pid: u32) -> Vec<(u32, &str)> {
        self.processes
            .iter()
            .filter(|process| process.pid == pid)
            .chain(self.descendants(pid))
            .map(|process| (process.pid, process.command.as_str()))
            .collect()
    }

    /// The command line of process `pid`, if it is running.
    pub(crate) fn command(&self, pid: u32) -> Option<&str> {
        self.processes
            .iter()
            .find(|process| process.pid == pid)
            .map(|process| process.command.as_str())
    }

    /// Every process below `pid`, parents before their children.
    fn descendants(&self, pid: u32) -> Vec<&Process> {
        let mut found = Vec::new();
        let mut pending = vec![pid];
        while let Some(parent) = pending.pop() {
            for &ix in self.children.get(&parent).into_iter().flatten() {
                let child = &self.processes[ix];
                found.push(child);
                pending.push(child.pid);
            }
        }
        found
    }
}

/// Whether the command line `command` runs `program`: directly, or as a
/// script handed to Node.js or Bun. Pi also runs from its package's `cli.js`.
fn runs(command: &str, program: &str) -> bool {
    let mut words = command.split_whitespace();
    let Some(first) = words.next().map(base_name) else {
        return false;
    };
    let script = matches!(first, "node" | "bun")
        .then(|| words.next())
        .flatten();
    first == program
        || script.is_some_and(|script| {
            base_name(script) == program
                || (program == "pi"
                    && script.contains("pi-coding-agent/")
                    && script.ends_with("/cli.js"))
        })
}

/// Last component of a path.
fn base_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Memory in the largest fitting unit, such as `312 MB` or `1.2 GB`.
pub fn format_memory(bytes: u64) -> String {
    const MB: f64 = 1024. * 1024.;
    let megabytes = bytes as f64 / MB;
    if megabytes >= 1024. {
        format!("{:.1} GB", megabytes / 1024.)
    } else {
        format!("{megabytes:.0} MB")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTPUT: &str = "    1     0  1024   0.5 /sbin/launchd\n\
                          100     1  2048  10.0 /bin/zsh -i\n\
                          101   100  1024   2.5 /usr/bin/node /opt/pi-coding-agent/dist/bundle/cli.js\n\
                          102   101   512   1.0 /bin/sh -c sleep\n\
                          200     1  4096   0.0 /bin/bash\n\
                          201   200  1024   0.0 /bin/zsh -i\n\
                          202   201  8192   3.0 opencode\n\
                          garbage line\n";

    #[test]
    fn sums_a_process_tree() {
        let table = ProcessTable::parse(OUTPUT);
        assert_eq!(
            table.tree(100),
            Some(Usage {
                memory: (2048 + 1024 + 512) * 1024,
                cpu: 13.5,
            })
        );
        assert_eq!(
            table.process(100).map(|usage| usage.memory),
            Some(2048 * 1024)
        );
        assert_eq!(table.tree(999), None);
    }

    #[test]
    fn lists_a_process_and_everything_below_it() {
        let table = ProcessTable::parse(OUTPUT);
        let mut pids: Vec<u32> = table.family(100).into_iter().map(|(pid, _)| pid).collect();
        pids.sort_unstable();
        assert_eq!(pids, [100, 101, 102]);
        assert_eq!(table.family(102), [(102, "/bin/sh -c sleep")]);
        assert!(table.family(999).is_empty());
        assert_eq!(table.command(202), Some("opencode"));
        assert_eq!(table.command(999), None);
    }

    #[test]
    fn finds_the_agent_running_below_a_terminal() {
        let table = ProcessTable::parse(OUTPUT);
        let agent = |id: &str, program: &str| Agent {
            id: id.into(),
            name: id.into(),
            command: vec![program.into()],
        };
        let shell = Agent {
            id: "shell".into(),
            name: "Shell".into(),
            command: Vec::new(),
        };
        let agents = [
            shell,
            agent("pi", "pi"),
            agent("opencode", "/opt/bin/opencode"),
        ];
        let found = |pid| {
            table
                .agent_below(pid, &agents)
                .map(|agent| agent.id.as_str())
        };
        assert_eq!(found(100), Some("pi"));
        assert_eq!(found(200), Some("opencode"));
        assert_eq!(found(102), None);
    }

    #[test]
    fn recognises_programs_without_matching_their_arguments() {
        assert!(runs("/opt/bin/pi --session /tmp/task.jsonl", "pi"));
        assert!(runs("node /opt/node/bin/pi", "pi"));
        assert!(runs(
            "/usr/bin/node /opt/pi-coding-agent/dist/bundle/cli.js",
            "pi"
        ));
        assert!(runs("opencode", "opencode"));
        assert!(!runs("/bin/zsh -i", "pi"));
        assert!(!runs("/usr/bin/node /opt/copy/cli.js", "pi"));
        assert!(!runs("grep pi", "pi"));
        assert!(!runs("/bin/sh -c if opencode; then x; fi", "opencode"));
    }

    #[test]
    fn reads_running_processes() {
        let table = ProcessTable::read().unwrap();
        let own = table.process(std::process::id()).unwrap();
        assert!(own.memory > 0);
    }

    #[test]
    fn formats_memory() {
        assert_eq!(format_memory(312 * 1024 * 1024), "312 MB");
        assert_eq!(format_memory(1536 * 1024 * 1024), "1.5 GB");
        assert_eq!(format_memory(0), "0 MB");
    }
}
