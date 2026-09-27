//! The [`SourceScanner`] port: a Source is cloned **shallow** with the
//! credentials of the user running the daemon (through the workspace's git
//! adapter), or opened in place when it is a local folder, then walked.
//!
//! The clone lives in a temp directory kept for a while, keyed by the
//! client-chosen `scan_id`, so the import that follows copies from it without
//! a second clone; an expired or unknown id simply re-scans.

mod scan_dir;

pub use scan_dir::scan_dir;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use pdo_workspace::adapters::git::clone::{clone_shallow, CloneError};
use pdo_workspace::adapters::git::repository::resolve_commit;

use crate::adapters::fs::hash_tree;
use crate::application::ports::{BoxFuture, KeptScan, SourceScanner};
use crate::domain::import::{parents_of, Candidate, ScanOutcome};
use crate::domain::import_error::{ImportError, CLONE_TIMEOUT};
use crate::domain::skill::SKILL_MD;
use crate::domain::source::{ParsedSource, SourceKind};
use crate::domain::update::TreeHashes;

/// How long a scan's clone stays on disk for the import that follows.
const SCAN_TTL: Duration = Duration::from_secs(30 * 60);

/// The clone (or the local folder) a scan produced, kept for the import.
struct ScanEntry {
    kept: KeptScan,
    /// A temp clone to delete on expiry; a local folder is never deleted.
    owned: bool,
    created: Instant,
}

/// The scans of this daemon: the kept checkouts, and the clones in flight so
/// a cancel can kill the child.
#[derive(Default)]
pub struct GitSourceScanner {
    scans: Mutex<HashMap<String, ScanEntry>>,
    in_flight: Mutex<HashMap<String, tokio::sync::oneshot::Sender<()>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn cleanup_entry(entry: &ScanEntry) {
    if entry.owned {
        let _ = std::fs::remove_dir_all(&entry.kept.root);
    }
}

/// Scoped by process: the scan id is chosen by the client, and two daemons on
/// one machine (or two test daemons) must never share a clone.
fn scan_temp_dir(scan_id: &str) -> PathBuf {
    std::env::temp_dir().join(format!("pdo-skill-scan-{}-{scan_id}", std::process::id()))
}

impl GitSourceScanner {
    pub fn new() -> Self {
        Self::default()
    }

    fn expire_scans(&self) {
        let mut guard = lock(&self.scans);
        let expired: Vec<String> = guard
            .iter()
            .filter(|(_, e)| e.created.elapsed() > SCAN_TTL)
            .map(|(k, _)| k.clone())
            .collect();
        for key in expired {
            if let Some(entry) = guard.remove(&key) {
                cleanup_entry(&entry);
            }
        }
        // Clones left behind by a previous daemon process (the map is
        // per-process): anything named like ours, older than the TTL and not
        // tracked, goes too.
        let tracked: HashSet<PathBuf> = guard.values().map(|e| e.kept.root.clone()).collect();
        if let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if !name.starts_with("pdo-skill-scan-") || tracked.contains(&path) {
                    continue;
                }
                let stale = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|m| m.elapsed().ok())
                    .is_some_and(|age| age > SCAN_TTL);
                if stale {
                    let _ = std::fs::remove_dir_all(&path);
                }
            }
        }
    }

    /// Shallow-clone `source` into a temp dir named after `scan_id`.
    async fn clone(&self, scan_id: &str, source: &ParsedSource) -> Result<PathBuf, ImportError> {
        let dir = scan_temp_dir(scan_id);
        let _ = std::fs::remove_dir_all(&dir);
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        lock(&self.in_flight).insert(scan_id.to_string(), tx);
        let outcome = clone_shallow(
            &source.url,
            source.git_ref.as_deref(),
            &dir,
            rx,
            CLONE_TIMEOUT,
        )
        .await
        .map(|()| dir.clone())
        .map_err(|error| match error {
            CloneError::Failed(stderr) => ImportError::CloneFailed(stderr),
            CloneError::Timeout => ImportError::CloneTimeout,
            CloneError::Cancelled => ImportError::Cancelled,
            CloneError::Io(message) => ImportError::Io(message),
        });
        lock(&self.in_flight).remove(scan_id);
        if outcome.is_err() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        outcome
    }

    async fn scan_now(
        &self,
        scan_id: &str,
        source: &ParsedSource,
    ) -> Result<ScanOutcome, ImportError> {
        self.expire_scans();
        // Drop any previous clone under the same id.
        if let Some(previous) = lock(&self.scans).remove(scan_id) {
            cleanup_entry(&previous);
        }
        let (root, owned) = match source.kind {
            SourceKind::Git => (self.clone(scan_id, source).await?, true),
            SourceKind::Local => {
                let path = PathBuf::from(&source.url);
                if !path.is_dir() {
                    return Err(ImportError::LocalNotFound(source.url.clone()));
                }
                (path, false)
            }
        };
        let head = root.clone();
        let commit = tokio::task::spawn_blocking(move || resolve_commit(&head, "HEAD"))
            .await
            .ok()
            .flatten();
        let candidates = scan_dir(&root, &source.path);
        let (elsewhere, elsewhere_count) = if candidates.is_empty() && !source.path.is_empty() {
            let all = scan_dir(&root, "");
            (parents_of(&all), all.len())
        } else {
            (Vec::new(), 0)
        };
        lock(&self.scans).insert(
            scan_id.to_string(),
            ScanEntry {
                kept: KeptScan {
                    root,
                    source: source.clone(),
                    commit: commit.clone(),
                },
                owned,
                created: Instant::now(),
            },
        );
        Ok(ScanOutcome {
            source: source.clone(),
            commit,
            candidates,
            elsewhere,
            elsewhere_count,
        })
    }
}

impl SourceScanner for GitSourceScanner {
    fn scan<'a>(
        &'a self,
        scan_id: &'a str,
        source: &'a ParsedSource,
    ) -> BoxFuture<'a, Result<ScanOutcome, ImportError>> {
        Box::pin(self.scan_now(scan_id, source))
    }

    fn cancel(&self, scan_id: &str) -> bool {
        match lock(&self.in_flight).remove(scan_id) {
            Some(tx) => {
                let _ = tx.send(());
                true
            }
            None => false,
        }
    }

    fn kept(&self, scan_id: &str) -> Option<KeptScan> {
        self.expire_scans();
        lock(&self.scans)
            .get(scan_id)
            .filter(|e| e.kept.root.is_dir())
            .map(|e| e.kept.clone())
    }

    fn candidates(&self, root: &Path, sub_path: &str) -> Vec<Candidate> {
        scan_dir(root, sub_path)
    }

    fn candidate_dir(&self, root: &Path, path: &str) -> Result<PathBuf, ImportError> {
        if path.split('/').any(|seg| seg == "..") {
            return Err(ImportError::UnknownCandidate(path.to_string()));
        }
        let dir = if path.is_empty() {
            root.to_path_buf()
        } else {
            root.join(path)
        };
        if !dir.join(SKILL_MD).is_file() {
            return Err(ImportError::UnknownCandidate(path.to_string()));
        }
        Ok(dir)
    }

    fn read_skill_md(&self, dir: &Path) -> std::io::Result<String> {
        std::fs::read_to_string(dir.join(SKILL_MD))
    }

    fn hashes(&self, dir: &Path) -> TreeHashes {
        hash_tree(dir)
    }
}
