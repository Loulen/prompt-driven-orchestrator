//! The pipeline watcher the daemon runs, behind the [`RunDirWatch`] port: a
//! Run directory is watched individually and non-recursively, so each new one
//! is registered with it.

use std::path::Path;
use std::sync::{Arc, Mutex};

use pdo_pipeline::adapters::watcher::{watch_run_dir, PipelineDebouncer};

use crate::application::ports::RunDirWatch;

/// The daemon's watcher; `None` when it could not start (or in a test).
pub struct DebouncedRunDirs(pub Arc<Mutex<Option<PipelineDebouncer>>>);

impl RunDirWatch for DebouncedRunDirs {
    fn watch(&self, run_dir: &Path) {
        let mut guard = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(debouncer) = guard.as_mut() {
            watch_run_dir(debouncer, run_dir);
        }
    }
}
