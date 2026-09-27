//! The file watcher's backends: native first, polling for any path the
//! native backend cannot register.

use std::path::Path;
use std::time::Duration;

use tracing::warn;

pub(super) const DEBOUNCE: Duration = Duration::from_secs(1);
/// Scan interval of the polling fallback. Combined with the debounce window,
/// worst-case detection latency stays well under the 4s the integration tests
/// (and a human waiting on hot-reload) tolerate.
pub(super) const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// File watcher with a graceful degradation path: prefer the OS-native
/// backend (inotify on Linux), and fall back to a 1s polling watcher for any
/// path the native backend cannot register — typically because the per-user
/// inotify watch pool (`fs.inotify.max_user_watches`) has been exhausted by
/// other processes (editors and Electron apps routinely pin hundreds of
/// thousands of watches). Without the fallback, a failed `watch()` silently
/// disabled hot-reload and external-edit detection for that path.
///
/// Every watched path is small (the instance pipelines dir, a run dir, a prompts dir),
/// so polling them is cheap; each path is registered with exactly one
/// backend, so no event is ever delivered twice.
pub struct PipelineDebouncer {
    pub(super) native: Option<notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>>,
    pub(super) poll: Option<notify_debouncer_mini::Debouncer<notify::PollWatcher>>,
}

impl PipelineDebouncer {
    pub(super) fn watch(
        &mut self,
        path: &Path,
        mode: notify::RecursiveMode,
    ) -> Result<(), notify::Error> {
        let mut native_err = None;
        if let Some(d) = self.native.as_mut() {
            match d.watcher().watch(path, mode) {
                Ok(()) => return Ok(()),
                Err(e) => native_err = Some(e),
            }
        }
        if let Some(d) = self.poll.as_mut() {
            let result = d.watcher().watch(path, mode);
            if result.is_ok() {
                if let Some(e) = native_err {
                    warn!(
                        "Native file watch failed for {} ({e}); falling back to {}s polling",
                        path.display(),
                        POLL_INTERVAL.as_secs()
                    );
                }
            }
            return result;
        }
        Err(native_err.unwrap_or_else(|| notify::Error::generic("no watcher backend available")))
    }
}

/// Watch a single run directory for the only run-scoped files the daemon
/// reacts to: `<run>/pipeline.yaml` and `<run>/pipeline.prompts/*.md`. Both
/// watches are non-recursive on purpose — a run dir also contains the
/// pipeline worktree and per-node sub-worktrees (`target/`, `node_modules/`,
/// `.git/`, ...). Watching the runs tree recursively used to pin tens of
/// thousands of inotify watches per run, exhausting the per-user
/// `max_user_watches` limit and silently breaking every other watcher on the
/// machine (including freshly spawned daemons, whose `watch()` calls then
/// fail with ENOSPC).
pub fn watch_run_dir(debouncer: &mut PipelineDebouncer, run_dir: &Path) {
    if let Err(e) = debouncer.watch(run_dir, notify::RecursiveMode::NonRecursive) {
        warn!("Failed to watch run dir {}: {e}", run_dir.display());
        return;
    }
    let prompts_dir = run_dir.join("pipeline.prompts");
    if prompts_dir.is_dir() {
        if let Err(e) = debouncer.watch(&prompts_dir, notify::RecursiveMode::NonRecursive) {
            warn!(
                "Failed to watch run prompts dir {}: {e}",
                prompts_dir.display()
            );
        }
    }
}
