//! The writes the daemon makes to a pipeline file on its own behalf. The
//! watcher reads them to tell the daemon's own saves from an outside edit.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// TTL during which a recorded self-write suppresses watcher broadcasts.
/// Larger than the debouncer interval (1s) with margin for filesystem latency.
pub const SELF_WRITE_TTL: Duration = Duration::from_secs(2);

/// Entries older than this are GCed when a new write is recorded.
const SELF_WRITE_GC: Duration = Duration::from_secs(5);

/// The paths the daemon wrote recently, shared by every writer and the
/// watcher (a clone shares the same record).
#[derive(Clone, Default)]
pub struct SelfWrites(Arc<Mutex<HashMap<PathBuf, Instant>>>);

impl SelfWrites {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a path the daemon is about to write so the file watcher can skip
    /// its own broadcast when notify reports the change. Call this *before*
    /// the write — the watcher otherwise races against the insert.
    pub fn mark(&self, path: &Path) {
        let now = Instant::now();
        let mut guard = match self.0.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.retain(|_, t| now.duration_since(*t) < SELF_WRITE_GC);
        guard.insert(path.to_path_buf(), now);
    }

    /// Whether `path` was marked less than [`SELF_WRITE_TTL`] ago.
    pub fn is_recent(&self, path: &Path) -> bool {
        let guard = match self.0.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard
            .get(path)
            .is_some_and(|t| t.elapsed() < SELF_WRITE_TTL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_marked_path_is_recent_and_another_is_not() {
        let writes = SelfWrites::new();
        writes.mark(Path::new("/p/a.yaml"));
        assert!(writes.is_recent(Path::new("/p/a.yaml")));
        assert!(!writes.is_recent(Path::new("/p/b.yaml")));
        assert!(writes.clone().is_recent(Path::new("/p/a.yaml")));
    }
}
