//! Saving and restoring state as JSON files.

use std::io;
use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;

/// Reads the value saved at `path`; a missing file means the default value.
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> io::Result<T> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(error),
    }
}

/// Writes `value` to `path`, replacing the old file only once the new one is
/// complete.
pub fn save<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    let partial = path.with_extension("json.partial");
    std::fs::write(&partial, json)?;
    std::fs::rename(&partial, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Workspace;
    use crate::test_support::TempDir;

    #[test]
    fn saves_and_loads() {
        let temp = TempDir::new();
        let path = temp.path().join("nested/state.json");
        assert_eq!(load::<Workspace>(&path).unwrap(), Workspace::default());

        let mut workspace = Workspace::default();
        workspace.add_project("/repo".into());
        workspace.add_session(
            Path::new("/repo"),
            "/repo".into(),
            "claude",
            vec!["--resume".into(), "abc".into()],
        );
        save(&path, &workspace).unwrap();
        assert_eq!(load::<Workspace>(&path).unwrap(), workspace);
        assert!(!path.with_extension("json.partial").exists());
    }

    #[test]
    fn corrupt_state_is_an_error() {
        let temp = TempDir::new();
        let path = temp.path().join("state.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert!(load::<Workspace>(&path).is_err());
    }
}
