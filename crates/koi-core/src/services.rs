//! Background services started by agents: servers and watchers that run
//! apart from the agent's terminal, found so they can be listed and stopped.
//!
//! A process started with `nohup` or detached from its shell is handed to
//! `launchd` and no longer runs below the terminal. Every session's
//! environment therefore carries a [`Tag`], which such processes inherit.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io;
use std::process::Command;
use std::time::Duration;

use crate::resources::{ProcessTable, Usage, runs};
use crate::{Agent, SessionId, process_stop};

/// Environment variable holding the [`Tag`] of the session a process
/// belongs to.
pub const SESSION_ENV: &str = "KOI_SESSION";

/// Name of Koi's executable, telling a running Koi from a reused pid.
const KOI_PROGRAM: &str = "koi";

/// Marks the processes of one session of one running Koi, so that two Koi
/// instances, or one restarted after a crash, keep their services apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Tag {
    /// Process id of the Koi that started the session.
    pub app: u32,
    pub session: SessionId,
}

impl Tag {
    /// The tag of `session` in this Koi.
    pub fn own(session: SessionId) -> Self {
        Self {
            app: std::process::id(),
            session,
        }
    }

    /// Reads a tag written by [`fmt::Display`].
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let (app, session) = text.split_once(':')?;
        Some(Self {
            app: app.parse().ok()?,
            session: session.parse().ok()?,
        })
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.app, self.session)
    }
}

/// A session as Koi runs it: its tag and its terminal's process, if the
/// terminal is still open.
#[derive(Clone, Copy, Debug)]
pub struct Owner {
    pub tag: Tag,
    pub root: Option<u32>,
}

/// TCP ports each process listens on.
pub type Ports = HashMap<u32, Vec<u16>>;

/// A background service and everything it started.
#[derive(Clone, Debug, PartialEq)]
pub struct Service {
    pub pid: u32,
    pub command: String,
    /// Ports it or a process below it listens on, ascending.
    pub ports: Vec<u16>,
    pub usage: Usage,
}

/// Where a group of services came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// A session of this Koi.
    Session(SessionId),
    /// Sessions of a Koi that is no longer running.
    Leftover,
}

/// The services of one source.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub source: Source,
    pub services: Vec<Service>,
}

/// Lists the TCP ports processes listen on, with `lsof`.
pub fn listening_ports() -> io::Result<Ports> {
    let output = Command::new("lsof")
        .args(["-nP", "-iTCP", "-sTCP:LISTEN", "-Fpn"])
        .output()?;
    // `lsof` fails when nothing listens; its output is then empty.
    Ok(parse_ports(&String::from_utf8_lossy(&output.stdout)))
}

/// Parses `lsof -F pn` output: a `p<pid>` line, then `n<address>:<port>`
/// lines for its sockets. Other lines are skipped.
fn parse_ports(output: &str) -> Ports {
    let mut ports = Ports::new();
    let mut pid = None;
    for line in output.lines() {
        if let Some(found) = line.strip_prefix('p') {
            pid = found.parse().ok();
        } else if let (Some(pid), Some(address)) = (pid, line.strip_prefix('n'))
            && let Some(port) = address
                .rsplit_once(':')
                .and_then(|(_, port)| port.parse().ok())
        {
            let listed: &mut Vec<u16> = ports.entry(pid).or_default();
            if !listed.contains(&port) {
                listed.push(port);
            }
        }
    }
    ports
}

/// The processes a session started: its terminal's, if still open, and
/// every process carrying its tag. Stopping their families ends them all.
pub fn session_roots(table: &ProcessTable, owner: Owner) -> Vec<u32> {
    owner
        .root
        .into_iter()
        .chain(
            table
                .tags()
                .filter(|(_, tag)| *tag == Some(owner.tag))
                .map(|(pid, _)| pid),
        )
        .collect()
}

/// The background services of `owners`, sessions of the Koi with pid
/// `app`, and those left by a Koi that is no longer running. Sessions
/// without services are left out.
///
/// A service is a process of a session that has left the terminal, or one
/// that listens on a port, other than the terminal's own process and the
/// `agents` themselves. Processes below a service count as part of it.
pub fn find(
    table: &ProcessTable,
    ports: &Ports,
    owners: &[Owner],
    agents: &[Agent],
    app: u32,
) -> Vec<Group> {
    let mut groups: Vec<Group> = owners
        .iter()
        .map(|owner| Group {
            source: Source::Session(owner.tag.session),
            services: session_services(table, ports, *owner, agents),
        })
        .collect();
    let leftover: Vec<u32> = table
        .tags()
        .filter_map(|(pid, tag)| Some((pid, tag?)))
        .filter(|(_, tag)| tag.app != app && !is_koi(table, tag.app))
        .map(|(pid, _)| pid)
        .collect();
    groups.push(Group {
        source: Source::Leftover,
        services: services(table, ports, &family_of(table, &leftover)),
    });
    groups.retain(|group| !group.services.is_empty());
    groups
}

/// Stops `services` and everything below them. A service that ended, or
/// whose pid now runs something else, is skipped.
pub fn stop(services: &[Service], grace: Duration) -> io::Result<()> {
    let table = ProcessTable::read()?;
    let roots: Vec<u32> = services
        .iter()
        .filter(|service| table.command(service.pid) == Some(service.command.as_str()))
        .map(|service| service.pid)
        .collect();
    process_stop::stop(&process_stop::targets(&table, &roots), grace)
}

fn session_services(
    table: &ProcessTable,
    ports: &Ports,
    owner: Owner,
    agents: &[Agent],
) -> Vec<Service> {
    let attached = family_of(table, &owner.root.into_iter().collect::<Vec<_>>());
    let members = family_of(table, &session_roots(table, owner));
    let candidates: HashSet<u32> = members
        .into_iter()
        .filter(|pid| Some(*pid) != owner.root)
        .filter(|pid| !attached.contains(pid) || ports.contains_key(pid))
        .filter(|&pid| !is_agent(table, pid, agents))
        .collect();
    services(table, ports, &candidates)
}

/// The services among `candidates`: those whose parent is not one, each
/// with the ports and usage of its whole family. Ordered by pid.
fn services(table: &ProcessTable, ports: &Ports, candidates: &HashSet<u32>) -> Vec<Service> {
    let mut found: Vec<Service> = candidates
        .iter()
        .filter(|pid| {
            table
                .parent(**pid)
                .is_none_or(|parent| !candidates.contains(&parent))
        })
        .filter_map(|&pid| {
            let family = table.family(pid);
            let mut listening: Vec<u16> = family
                .iter()
                .flat_map(|(pid, _)| ports.get(pid).into_iter().flatten().copied())
                .collect();
            listening.sort_unstable();
            listening.dedup();
            Some(Service {
                pid,
                command: table.command(pid)?.to_owned(),
                ports: listening,
                usage: table.tree(pid)?,
            })
        })
        .collect();
    found.sort_unstable_by_key(|service| service.pid);
    found
}

/// The processes `roots` and every process below them.
fn family_of(table: &ProcessTable, roots: &[u32]) -> HashSet<u32> {
    roots
        .iter()
        .flat_map(|&root| table.family(root))
        .map(|(pid, _)| pid)
        .collect()
}

fn is_agent(table: &ProcessTable, pid: u32, agents: &[Agent]) -> bool {
    table.command(pid).is_some_and(|command| {
        agents
            .iter()
            .filter_map(Agent::program_name)
            .any(|program| runs(command, program))
    })
}

/// Whether process `pid` is a running Koi.
fn is_koi(table: &ProcessTable, pid: u32) -> bool {
    table
        .command(pid)
        .is_some_and(|command| runs(command, KOI_PROGRAM))
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP: u32 = 50;

    fn session(id: &str) -> SessionId {
        id.parse().unwrap()
    }

    /// Koi (50) runs session 1 in terminal 100: pi, a dev server it waits
    /// on, and a server it left running in the background (300, now below
    /// launchd). A crashed Koi (40) left a watcher; another Koi (60) runs one.
    fn table() -> ProcessTable {
        let mut table = ProcessTable::parse(
            "50 1 1 0.0 /Applications/Koi.app/Contents/MacOS/koi\n\
             60 1 1 0.0 /opt/koi\n\
             100 50 1 0.0 /bin/zsh -l\n\
             101 100 1 1.0 node /opt/pi-coding-agent/dist/cli.js\n\
             102 101 1 0.0 /bin/bash -c pnpm dev\n\
             103 102 1 2.0 node vite\n\
             104 101 1 0.0 node mcp-server.js\n\
             300 1 2 0.5 pnpm start\n\
             301 300 3 0.5 node server.js\n\
             400 1 1 0.0 cargo watch\n\
             500 1 1 0.0 node other.js\n\
             600 1 1 0.0 /usr/sbin/sshd\n",
        );
        table.read_tags(
            "100 /bin/zsh -l KOI_SESSION=50:1\n\
             101 node /opt/pi-coding-agent/dist/cli.js KOI_SESSION=50:1\n\
             102 /bin/bash -c pnpm dev KOI_SESSION=50:1\n\
             103 node vite KOI_SESSION=50:1\n\
             104 node mcp-server.js KOI_SESSION=50:1\n\
             300 pnpm start KOI_SESSION=50:1\n\
             301 node server.js KOI_SESSION=50:1\n\
             400 cargo watch KOI_SESSION=40:9\n\
             500 node other.js KOI_SESSION=60:2\n",
        );
        table
    }

    fn ports() -> Ports {
        Ports::from([(103, vec![5173]), (301, vec![3000, 3001]), (600, vec![22])])
    }

    fn owners() -> [Owner; 1] {
        [Owner {
            tag: Tag {
                app: APP,
                session: session("1"),
            },
            root: Some(100),
        }]
    }

    fn agents() -> [Agent; 1] {
        [Agent {
            id: "pi".into(),
            name: "Pi".into(),
            command: vec!["pi".into()],
        }]
    }

    fn pids(group: &Group) -> Vec<u32> {
        group.services.iter().map(|service| service.pid).collect()
    }

    #[test]
    fn tags_round_trip() {
        let tag = Tag {
            app: 7,
            session: session("12"),
        };
        assert_eq!(tag.to_string(), "7:12");
        assert_eq!(Tag::parse("7:12"), Some(tag));
        assert_eq!(Tag::parse("7"), None);
        assert_eq!(Tag::parse("x:12"), None);
    }

    #[test]
    fn parses_listening_ports() {
        let ports = parse_ports("p10\nf5\nn*:3000\nf6\nn[::1]:3000\nn127.0.0.1:9229\np11\nnbad\n");
        assert_eq!(ports, Ports::from([(10, vec![3000, 9229])]));
    }

    #[test]
    fn finds_detached_and_listening_services() {
        let groups = find(&table(), &ports(), &owners(), &agents(), APP);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].source, Source::Session(session("1")));
        // The MCP server neither left nor listens; the dev server's shell
        // holds no port itself.
        assert_eq!(pids(&groups[0]), [103, 300]);
        let background = &groups[0].services[1];
        assert_eq!(background.ports, [3000, 3001]);
        assert_eq!(background.usage.memory, 5 * 1024);
        assert_eq!(groups[1].source, Source::Leftover);
        assert_eq!(pids(&groups[1]), [400]);
    }

    #[test]
    fn roots_cover_the_terminal_and_tagged_processes() {
        let mut roots = session_roots(&table(), owners()[0]);
        roots.sort_unstable();
        assert_eq!(roots, [100, 100, 101, 102, 103, 104, 300, 301]);
    }
}
