use crate::git::{repo_paths, RepoPaths};
use anyhow::{Context, Result};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
    time::Duration,
};

const STOP_POLL_INTERVAL: Duration = Duration::from_millis(50);
const DEFAULT_DEBOUNCE_DELAY: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchMessage {
    RefreshRequested,
}

pub struct RepoWatcher {
    _watcher: RecommendedWatcher,
    stop_tx: Option<Sender<()>>,
    debounce_thread: Option<JoinHandle<()>>,
}

impl Drop for RepoWatcher {
    fn drop(&mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        if let Some(handle) = self.debounce_thread.take() {
            let _ = handle.join();
        }
    }
}

pub fn start_repo_watcher(root: &Path, tx: Sender<WatchMessage>) -> Result<RepoWatcher> {
    let repo_paths = repo_paths(root)?;
    let watch_targets = watch_targets(&repo_paths);
    let (raw_tx, raw_rx) = mpsc::channel::<Vec<PathBuf>>();
    let (stop_tx, stop_rx) = mpsc::channel();

    let mut watcher = RecommendedWatcher::new(
        move |result: notify::Result<Event>| {
            if let Ok(event) = result {
                let _ = raw_tx.send(event.paths);
            }
        },
        Config::default(),
    )
    .context("failed to initialize filesystem watcher")?;

    for (path, recursive_mode) in &watch_targets {
        watcher
            .watch(path, *recursive_mode)
            .with_context(|| format!("failed to watch {}", path.display()))?;
    }

    let debounce_thread = thread::spawn(move || {
        debounce_refresh_events(raw_rx, stop_rx, tx, repo_paths, DEFAULT_DEBOUNCE_DELAY);
    });

    Ok(RepoWatcher {
        _watcher: watcher,
        stop_tx: Some(stop_tx),
        debounce_thread: Some(debounce_thread),
    })
}

fn watch_targets(repo_paths: &RepoPaths) -> Vec<(PathBuf, RecursiveMode)> {
    let mut seen = HashSet::new();
    let mut targets = Vec::new();

    for (path, recursive_mode) in [
        (repo_paths.root.clone(), RecursiveMode::Recursive),
        (repo_paths.git_dir.clone(), RecursiveMode::Recursive),
        (repo_paths.git_common_dir.clone(), RecursiveMode::Recursive),
    ] {
        if seen.insert(path.clone()) {
            targets.push((path, recursive_mode));
        }
    }

    targets
}

fn debounce_refresh_events(
    raw_rx: Receiver<Vec<PathBuf>>,
    stop_rx: Receiver<()>,
    tx: Sender<WatchMessage>,
    repo_paths: RepoPaths,
    debounce_delay: Duration,
) {
    loop {
        if stop_rx.try_recv().is_ok() {
            return;
        }

        let mut pending_refresh = match raw_rx.recv_timeout(STOP_POLL_INTERVAL) {
            Ok(paths) => should_refresh_for_paths(&paths, &repo_paths),
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };

        loop {
            if stop_rx.try_recv().is_ok() {
                return;
            }

            match raw_rx.recv_timeout(debounce_delay) {
                Ok(paths) => {
                    pending_refresh |= should_refresh_for_paths(&paths, &repo_paths);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if pending_refresh {
                        let _ = tx.send(WatchMessage::RefreshRequested);
                    }
                    break;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if pending_refresh {
                        let _ = tx.send(WatchMessage::RefreshRequested);
                    }
                    return;
                }
            }
        }
    }
}

fn should_refresh_for_paths(paths: &[PathBuf], repo_paths: &RepoPaths) -> bool {
    paths
        .iter()
        .any(|path| should_refresh_for_path(path, repo_paths))
}

fn should_refresh_for_path(path: &Path, repo_paths: &RepoPaths) -> bool {
    if path_is_git_refresh_target(path, &repo_paths.git_dir)
        || path_is_git_refresh_target(path, &repo_paths.git_common_dir)
    {
        return true;
    }

    if let Ok(relative) = path.strip_prefix(&repo_paths.root) {
        if relative.as_os_str().is_empty() {
            return false;
        }
        let first = relative.iter().next();
        if first == Some(".git".as_ref()) || first == Some("target".as_ref()) {
            return false;
        }
        return true;
    }

    false
}

fn path_is_git_refresh_target(path: &Path, git_dir: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(git_dir) else {
        return false;
    };

    if relative.as_os_str().is_empty() {
        return false;
    }

    matches!(
        relative.to_string_lossy().as_ref(),
        "HEAD" | "index" | "packed-refs" | "logs/HEAD"
    ) || relative.starts_with("refs")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn repo_paths() -> RepoPaths {
        RepoPaths {
            root: PathBuf::from("/repo"),
            git_dir: PathBuf::from("/repo/.git"),
            git_common_dir: PathBuf::from("/repo/.git"),
        }
    }

    #[test]
    fn should_refresh_for_worktree_file_changes() {
        let repo_paths = repo_paths();
        assert!(should_refresh_for_path(
            Path::new("/repo/src/main.rs"),
            &repo_paths
        ));
        assert!(should_refresh_for_path(Path::new("/repo/notes.txt"), &repo_paths));
    }

    #[test]
    fn ignores_git_metadata_duplicates_and_build_output() {
        let repo_paths = repo_paths();
        assert!(!should_refresh_for_path(Path::new("/repo/.git"), &repo_paths));
        assert!(!should_refresh_for_path(
            Path::new("/repo/.git/index.lock"),
            &repo_paths
        ));
        assert!(!should_refresh_for_path(Path::new("/repo/target/debug/gl"), &repo_paths));
    }

    #[test]
    fn refreshes_for_head_index_and_refs_updates() {
        let repo_paths = repo_paths();
        assert!(should_refresh_for_path(Path::new("/repo/.git/HEAD"), &repo_paths));
        assert!(should_refresh_for_path(Path::new("/repo/.git/index"), &repo_paths));
        assert!(should_refresh_for_path(
            Path::new("/repo/.git/refs/heads/main"),
            &repo_paths
        ));
        assert!(should_refresh_for_path(
            Path::new("/repo/.git/packed-refs"),
            &repo_paths
        ));
    }

    #[test]
    fn debounce_coalesces_multiple_events_into_one_refresh() {
        let repo_paths = repo_paths();
        let (raw_tx, raw_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();
        let (watch_tx, watch_rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            debounce_refresh_events(
                raw_rx,
                stop_rx,
                watch_tx,
                repo_paths,
                Duration::from_millis(10),
            );
        });

        raw_tx.send(vec![PathBuf::from("/repo/src/main.rs")]).unwrap();
        raw_tx.send(vec![PathBuf::from("/repo/.git/index")]).unwrap();
        thread::sleep(Duration::from_millis(30));

        assert_eq!(watch_rx.recv_timeout(Duration::from_millis(50)).unwrap(), WatchMessage::RefreshRequested);
        assert!(watch_rx.recv_timeout(Duration::from_millis(20)).is_err());

        let _ = stop_tx.send(());
        let _ = handle.join();
    }
}
