//! User configuration, read from `config.toml`.
//!
//! ```toml
//! [[agents]]
//! id = "claude-opus"
//! name = "Claude (Opus)"
//! command = ["claude", "--model", "opus"]
//! ```
//!
//! An agent whose `id` matches a built-in one replaces it.

use std::fmt;
use std::io;
use std::path::Path;

use serde::Deserialize;

use crate::Agent;
use crate::agent::builtin_agents;

#[derive(Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    agents: Vec<Agent>,
}

/// A configuration file that could not be used.
#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Parse(toml::de::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "could not read the configuration: {error}"),
            Self::Parse(error) => write!(f, "invalid configuration: {error}"),
        }
    }
}

impl std::error::Error for Error {}

impl Config {
    /// Reads the configuration at `path`; a missing file means the defaults.
    pub fn load(path: &Path) -> Result<Self, Error> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(Error::Parse),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(Error::Io(error)),
        }
    }

    /// Built-in agents with configured ones applied: same `id` replaces,
    /// new ones are appended.
    pub fn agents(&self) -> Vec<Agent> {
        let mut agents = builtin_agents();
        for agent in &self.agents {
            match agents.iter_mut().find(|known| known.id == agent.id) {
                Some(known) => *known = agent.clone(),
                None => agents.push(agent.clone()),
            }
        }
        agents
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn missing_file_means_defaults() {
        let temp = TempDir::new();
        let config = Config::load(&temp.path().join("config.toml")).unwrap();
        assert_eq!(config.agents(), builtin_agents());
    }

    #[test]
    fn configured_agents_replace_and_extend() {
        let config: Config = toml::from_str(
            r#"
            [[agents]]
            id = "claude"
            name = "Claude (Opus)"
            command = ["claude", "--model", "opus"]

            [[agents]]
            id = "aider"
            name = "Aider"
            command = ["aider"]
            "#,
        )
        .unwrap();
        let agents = config.agents();
        assert_eq!(agents.len(), builtin_agents().len() + 1);
        assert_eq!(agents[0].command, ["claude", "--model", "opus"]);
        assert_eq!(agents.last().unwrap().id, "aider");
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let temp = TempDir::new();
        let path = temp.path().join("config.toml");
        std::fs::write(&path, "agnets = []").unwrap();
        assert!(matches!(Config::load(&path), Err(Error::Parse(_))));
    }
}
