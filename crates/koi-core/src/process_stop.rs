//! Ending everything a session started. Agents run servers and watchers in
//! the background, apart from the terminal, so closing the terminal alone
//! leaves them running.

use std::io;
use std::process::Command;
use std::time::{Duration, Instant};

use crate::resources::ProcessTable;

/// How often stopping processes are checked on.
const POLL: Duration = Duration::from_millis(100);

/// A process to stop. Its command line tells it apart from a later process
/// that is given the same pid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pid: u32,
    command: String,
}

/// The processes of the terminals running `roots`: each root and every
/// process below it, as `table` lists them.
pub fn targets(table: &ProcessTable, roots: &[u32]) -> Vec<Target> {
    roots
        .iter()
        .flat_map(|&root| table.family(root))
        .map(|(pid, command)| Target {
            pid,
            command: command.to_owned(),
        })
        .collect()
}

/// Asks `targets` to end, then kills those still running after `grace`.
/// Background services must be stopped before their terminal closes, as
/// they then no longer run below it.
pub fn stop(targets: &[Target], grace: Duration) -> io::Result<()> {
    signal("-TERM", targets)?;
    let deadline = Instant::now() + grace;
    loop {
        let left = still_running(&ProcessTable::read()?, targets);
        if left.is_empty() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return signal("-KILL", &left);
        }
        std::thread::sleep(POLL);
    }
}

/// The targets that `table` still lists with the same command line.
fn still_running(table: &ProcessTable, targets: &[Target]) -> Vec<Target> {
    targets
        .iter()
        .filter(|target| table.command(target.pid) == Some(target.command.as_str()))
        .cloned()
        .collect()
}

/// Sends `signal` to `targets` with `kill`. Processes that ended meanwhile
/// make `kill` fail, which is expected and ignored.
fn signal(signal: &str, targets: &[Target]) -> io::Result<()> {
    if targets.is_empty() {
        return Ok(());
    }
    Command::new("kill")
        .arg(signal)
        .args(targets.iter().map(|target| target.pid.to_string()))
        .output()
        .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Child, Stdio};

    fn table(output: &str) -> ProcessTable {
        ProcessTable::parse(output)
    }

    #[test]
    fn targets_a_terminal_and_everything_below_it() {
        let table = table(
            "100 1 1 0.0 /bin/zsh -i\n\
             101 100 1 0.0 node agent.js\n\
             102 101 1 0.0 pnpm dev\n\
             200 1 1 0.0 /bin/zsh -i\n",
        );
        let mut pids: Vec<u32> = targets(&table, &[100, 999])
            .into_iter()
            .map(|target| target.pid)
            .collect();
        pids.sort_unstable();
        assert_eq!(pids, [100, 101, 102]);
    }

    #[test]
    fn a_reused_pid_is_not_still_running() {
        let before = table("100 1 1 0.0 pnpm dev\n101 1 1 0.0 node server.js\n");
        let after = table("100 1 1 0.0 ssh host\n101 1 1 0.0 node server.js\n");
        let left = still_running(&after, &targets(&before, &[100, 101]));
        assert_eq!(
            left,
            [Target {
                pid: 101,
                command: "node server.js".into()
            }]
        );
    }

    fn spawn(script: &str) -> Child {
        Command::new("sh")
            .args(["-c", script])
            .stdin(Stdio::null())
            .spawn()
            .unwrap()
    }

    fn wait_listed(pid: u32) -> Vec<Target> {
        for _ in 0..50 {
            let found = targets(&ProcessTable::read().unwrap(), &[pid]);
            if found.len() > 1 {
                return found;
            }
            std::thread::sleep(POLL);
        }
        panic!("the test processes did not start");
    }

    #[test]
    fn stops_processes_and_kills_those_that_ignore_it() {
        // A child that ignores the request to end, below one that does not.
        let mut parent = spawn("sh -c 'trap \"\" TERM; sleep 30' & wait");
        let found = wait_listed(parent.id());
        stop(&found, Duration::from_millis(300)).unwrap();
        assert!(parent.wait().is_ok());
        assert!(still_running(&ProcessTable::read().unwrap(), &found).is_empty());
    }
}
