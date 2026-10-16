//! Watches directories for changes.
//!
//! File system events arrive on a watcher thread; they are collected here and
//! a [`Signal`] wakes the app, which then takes the changed paths.

use std::collections::{BTreeSet, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use koi_term::Signal;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

pub struct FsWatcher {
    changed: Arc<Mutex<BTreeSet<PathBuf>>>,
    signal: Signal,
    watched: HashSet<PathBuf>,
    watcher: RecommendedWatcher,
}

impl FsWatcher {
    pub fn new() -> io::Result<Self> {
        let changed = Arc::new(Mutex::new(BTreeSet::new()));
        let signal = Signal::default();
        let watcher = notify::recommended_watcher({
            let (changed, signal) = (changed.clone(), signal.clone());
            move |event: notify::Result<notify::Event>| {
                let Ok(event) = event else {
                    return;
                };
                if !matches!(event.kind, EventKind::Access(_)) {
                    changed
                        .lock()
                        .expect("watcher lock poisoned")
                        .extend(event.paths);
                    signal.notify();
                }
            }
        })
        .map_err(io::Error::other)?;
        Ok(Self {
            changed,
            signal,
            watched: HashSet::new(),
            watcher,
        })
    }

    /// Starts watching `dir` unless it is watched already. A directory that
    /// does not exist is skipped; call again once it does.
    pub fn watch(&mut self, dir: &Path, recursive: bool) -> io::Result<()> {
        if self.watched.contains(dir) || !dir.is_dir() {
            return Ok(());
        }
        let mode = if recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        self.watcher.watch(dir, mode).map_err(io::Error::other)?;
        self.watched.insert(dir.to_owned());
        Ok(())
    }

    /// Stops watching `dir` and everything below it.
    pub fn unwatch_all_in(&mut self, dir: &Path) {
        let inside: Vec<PathBuf> = self
            .watched
            .iter()
            .filter(|watched| watched.starts_with(dir))
            .cloned()
            .collect();
        for path in inside {
            // Fails only if the directory is already gone, which ends the watch anyway.
            let _ = self.watcher.unwatch(&path);
            self.watched.remove(&path);
        }
    }

    /// Raised when watched files change.
    pub fn signal(&self) -> Signal {
        self.signal.clone()
    }

    /// Removes and returns the paths changed since the last call.
    pub fn take_changed(&self) -> BTreeSet<PathBuf> {
        std::mem::take(&mut *self.changed.lock().expect("watcher lock poisoned"))
    }
}

#[cfg(test)]
mod tests {
    use super::FsWatcher;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    /// A fresh directory with symlinks resolved (/var -> /private/var), so
    /// its path compares equal to the paths in events.
    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("koi-watch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    /// Waits until the watcher reports a change under `dir`.
    fn saw_change(watcher: &FsWatcher, dir: &Path) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if watcher
                .take_changed()
                .iter()
                .any(|path| path.starts_with(dir))
            {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    #[test]
    fn reports_changes_until_unwatched() {
        let dir = temp_dir();
        let mut watcher = FsWatcher::new().unwrap();

        watcher.watch(&dir.join("missing"), false).unwrap();
        assert!(
            watcher.watched.is_empty(),
            "missing directories are skipped"
        );

        watcher.watch(&dir, false).unwrap();
        std::fs::write(dir.join("status"), "idle").unwrap();
        assert!(saw_change(&watcher, &dir));

        watcher.unwatch_all_in(&dir);
        assert!(watcher.watched.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
