//! Koi's domain logic: projects (git repositories or plain folders) and
//! their git worktrees, agents, user configuration and the persisted
//! workspace. No UI dependency.

mod activity;
mod agent;
pub mod change_marks;
pub mod config;
pub mod file_language;
pub mod file_preview;
pub mod file_tabs;
pub mod file_tree;
mod fuzzy;
pub mod git;
pub mod history;
pub mod integration;
pub mod layout;
pub mod line_changes;
pub mod opencode;
mod paths;
pub mod process_stop;
pub mod project_files;
pub mod resources;
pub mod session_label;
pub mod settings;
pub mod store;
mod workspace;

#[cfg(test)]
mod test_support;

pub use activity::{Activity, Attention, reentry_change, unseen_after_change};
pub use agent::{Agent, shell_command};
pub use config::Config;
pub use fuzzy::{FuzzyMatch, fuzzy_match};
pub use paths::AppPaths;
pub use workspace::{Project, SessionId, SessionRecord, Split, SplitSide, Workspace, cycle};
