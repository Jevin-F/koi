//! Where Koi keeps its files.

use std::path::{Path, PathBuf};

use crate::SessionId;

/// Koi's data directory and the files inside it.
#[derive(Clone, Debug)]
pub struct AppPaths {
    home: PathBuf,
    root: PathBuf,
}

impl AppPaths {
    /// The standard macOS location under `home`.
    pub fn in_home(home: &Path) -> Self {
        Self {
            home: home.to_owned(),
            root: home.join("Library/Application Support/Koi"),
        }
    }

    /// The user's home directory, where agents keep their own records.
    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn config_file(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    pub fn state_file(&self) -> PathBuf {
        self.root.join("state.json")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    /// Directory of per-session status files, written by agent integrations.
    pub fn status_dir(&self) -> PathBuf {
        self.root.join("status")
    }

    /// The status file of one session.
    pub fn status_file(&self, id: SessionId) -> PathBuf {
        self.status_dir().join(id.to_string())
    }

    /// A prompt excerpt reported by an agent for automatic session naming.
    pub fn title_file(&self, id: SessionId) -> PathBuf {
        self.status_dir().join(format!("{id}.title"))
    }

    /// A session file path reported by an agent for future restarts.
    pub fn resume_file(&self, id: SessionId) -> PathBuf {
        self.status_dir().join(format!("{id}.resume"))
    }

    /// The session a status file belongs to.
    pub fn session_of_status_file(path: &Path) -> Option<SessionId> {
        path.file_name()?.to_str()?.parse().ok()
    }

    /// The session a prompt title file belongs to.
    pub fn session_of_title_file(path: &Path) -> Option<SessionId> {
        path.file_name()?
            .to_str()?
            .strip_suffix(".title")?
            .parse()
            .ok()
    }

    /// The session an agent's resume report belongs to.
    pub fn session_of_resume_file(path: &Path) -> Option<SessionId> {
        path.file_name()?
            .to_str()?
            .strip_suffix(".resume")?
            .parse()
            .ok()
    }

    /// Where the pi extension that reports activity is installed.
    pub fn pi_extension_file(&self) -> PathBuf {
        self.root.join("integrations/pi-status.ts")
    }

    /// Where a new worktree of `project` for `branch` is created.
    pub fn worktree_dir(&self, project: &Path, branch: &str) -> PathBuf {
        let project_name = project
            .file_name()
            .map_or_else(|| "project".into(), |name| name.to_string_lossy());
        self.root
            .join("worktrees")
            .join(sanitize(&project_name))
            .join(sanitize(branch))
    }
}

/// Makes `name` a single safe path component.
fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('.');
    if trimmed.is_empty() {
        "-".to_owned()
    } else {
        trimmed.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_live_in_application_support() {
        let paths = AppPaths::in_home(Path::new("/h"));
        assert_eq!(
            paths.state_file(),
            Path::new("/h/Library/Application Support/Koi/state.json")
        );
    }

    #[test]
    fn status_files_name_their_session() {
        let paths = AppPaths::in_home(Path::new("/h"));
        let id: SessionId = "7".parse().unwrap();
        assert_eq!(
            AppPaths::session_of_status_file(&paths.status_file(id)),
            Some(id)
        );
        assert_eq!(
            AppPaths::session_of_status_file(Path::new("/h/notes.txt")),
            None
        );
        assert_eq!(
            AppPaths::session_of_title_file(&paths.title_file(id)),
            Some(id)
        );
        assert_eq!(
            AppPaths::session_of_title_file(&paths.status_file(id)),
            None
        );
        assert_eq!(
            AppPaths::session_of_resume_file(&paths.resume_file(id)),
            Some(id)
        );
        assert_eq!(
            AppPaths::session_of_resume_file(&paths.status_file(id)),
            None
        );
    }

    #[test]
    fn worktree_dirs_are_flat_and_safe() {
        let paths = AppPaths::in_home(Path::new("/h"));
        assert_eq!(
            paths.worktree_dir(Path::new("/code/my app"), "feature/login"),
            Path::new("/h/Library/Application Support/Koi/worktrees/my-app/feature-login")
        );
        assert_eq!(sanitize(".."), "-");
        assert_eq!(sanitize("修复"), "修复");
    }
}
