use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use notify_debouncer_mini::{new_debouncer, new_debouncer_opt, DebouncedEventKind};
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::SELF_WRITE_TTL;

/// Debounce window for both backends.
const DEBOUNCE: Duration = Duration::from_secs(1);
/// Scan interval of the polling fallback. Combined with the debounce window,
/// worst-case detection latency stays well under the 4s the integration tests
/// (and a human waiting on hot-reload) tolerate.
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// File watcher with a graceful degradation path: prefer the OS-native
/// backend (inotify on Linux), and fall back to a 1s polling watcher for any
/// path the native backend cannot register — typically because the per-user
/// inotify watch pool (`fs.inotify.max_user_watches`) has been exhausted by
/// other processes (editors and Electron apps routinely pin hundreds of
/// thousands of watches). Without the fallback, a failed `watch()` silently
/// disabled hot-reload and external-edit detection for that path.
///
/// Every watched path is small (a pipelines dir, a run dir, a prompts dir),
/// so polling them is cheap; each path is registered with exactly one
/// backend, so no event is ever delivered twice.
pub struct PipelineDebouncer {
    native: Option<notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>>,
    poll: Option<notify_debouncer_mini::Debouncer<notify::PollWatcher>>,
}

impl PipelineDebouncer {
    fn watch(&mut self, path: &Path, mode: notify::RecursiveMode) -> Result<(), notify::Error> {
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

#[derive(Debug, Clone)]
pub struct RunPipelineModified {
    pub run_id: String,
    pub kind: &'static str, // "yaml" or "prompt"
    pub path: PathBuf,
}

pub fn spawn_watcher(
    repo_root: PathBuf,
    event_tx: broadcast::Sender<serde_json::Value>,
    recent_writes: Arc<Mutex<HashMap<PathBuf, Instant>>>,
    run_modified_tx: tokio::sync::mpsc::UnboundedSender<RunPipelineModified>,
) -> Option<PipelineDebouncer> {
    let repo_pipelines_dir = repo_root.join(".maestro").join("pipelines");
    let runs_dir = repo_root.join(".maestro").join("runs");
    let user_pipelines_dir = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|h| h.join(".maestro").join("pipelines"));

    let tx = Arc::new(event_tx);
    let mtimes: Arc<Mutex<HashMap<PathBuf, SystemTime>>> = Arc::new(Mutex::new(HashMap::new()));
    seed_mtimes(&mtimes, &repo_pipelines_dir);
    if let Some(ref user_dir) = user_pipelines_dir {
        seed_mtimes(&mtimes, user_dir);
    }
    seed_run_mtimes(&mtimes, &runs_dir);

    let runs_dir_for_closure = runs_dir.clone();

    // One shared handler, fed by both backends (a given path only ever
    // reports through the backend that registered it).
    type DebounceResult = Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>;
    let handler: Arc<dyn Fn(DebounceResult) + Send + Sync> = Arc::new(
        move |events: DebounceResult| {
            let Ok(events) = events else { return };
            for event in events {
                if event.kind != DebouncedEventKind::Any {
                    continue;
                }
                let path = &event.path;
                let ext = path.extension().and_then(|e| e.to_str());
                if ext != Some("yaml") && ext != Some("md") {
                    continue;
                }

                if !content_actually_changed(&mtimes, path) {
                    continue;
                }

                if is_recent_self_write(&recent_writes, path) {
                    info!(
                        "Pipeline file changed (self-write, suppressed): {}",
                        path.display()
                    );
                    continue;
                }

                // Detect run-scoped pipeline changes
                if path.starts_with(&runs_dir_for_closure) {
                    if let Some(modified) = detect_run_scoped_change(path, &runs_dir_for_closure) {
                        info!(
                            "Run-scoped pipeline modified: run={} kind={} path={}",
                            modified.run_id,
                            modified.kind,
                            modified.path.display()
                        );
                        let _ = run_modified_tx.send(modified);
                    }
                    continue;
                }

                let pipeline_id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();

                let pipeline_id = if ext == Some("md") {
                    path.parent()
                        .and_then(|p| p.file_name())
                        .and_then(|n| n.to_str())
                        .and_then(|n| n.strip_suffix(".prompts"))
                        .unwrap_or(&pipeline_id)
                        .to_string()
                } else {
                    pipeline_id
                };

                let msg = serde_json::json!({
                    "type": "pipeline_changed",
                    "pipeline_id": pipeline_id,
                    "path": path.to_string_lossy(),
                });

                let _ = tx.send(msg);
                info!("Pipeline file changed: {}", path.display());
            }
        },
    );

    let native = match new_debouncer(DEBOUNCE, {
        let handler = handler.clone();
        move |events: DebounceResult| handler(events)
    }) {
        Ok(d) => Some(d),
        Err(e) => {
            warn!("Failed to create native file watcher: {e}");
            None
        }
    };

    let poll = match new_debouncer_opt::<_, notify::PollWatcher>(
        notify_debouncer_mini::Config::default()
            .with_timeout(DEBOUNCE)
            .with_notify_config(
                notify::Config::default()
                    .with_poll_interval(POLL_INTERVAL)
                    // The poll backend stores mtimes at *second* granularity;
                    // without content comparison, a write landing in the same
                    // wall-clock second as the previous scan is never seen.
                    // The watched sets are tiny (a few KB of YAML/MD), so
                    // hashing them every scan is negligible.
                    .with_compare_contents(true),
            ),
        {
            let handler = handler.clone();
            move |events: DebounceResult| handler(events)
        },
    ) {
        Ok(d) => Some(d),
        Err(e) => {
            warn!("Failed to create polling file watcher: {e}");
            None
        }
    };

    if native.is_none() && poll.is_none() {
        warn!("Failed to create file watcher: no backend available");
        return None;
    }
    let mut debouncer = PipelineDebouncer { native, poll };

    if !repo_pipelines_dir.exists() {
        let _ = std::fs::create_dir_all(&repo_pipelines_dir);
    }
    if let Err(e) = debouncer.watch(&repo_pipelines_dir, notify::RecursiveMode::Recursive) {
        warn!("Failed to watch repo pipelines dir: {e}");
    } else {
        info!("Watching repo pipelines: {}", repo_pipelines_dir.display());
    }

    if let Some(ref user_dir) = user_pipelines_dir {
        if user_dir.exists() {
            if let Err(e) = debouncer.watch(user_dir, notify::RecursiveMode::Recursive) {
                warn!("Failed to watch user pipelines dir: {e}");
            } else {
                info!("Watching user pipelines: {}", user_dir.display());
            }
        }
    }

    // Watch run dirs for run-scoped pipeline edits. Each run dir is watched
    // individually and non-recursively (see `watch_run_dir`); runs created
    // after boot are registered by the run-creation handler.
    if !runs_dir.exists() {
        let _ = std::fs::create_dir_all(&runs_dir);
    }
    if let Ok(entries) = std::fs::read_dir(&runs_dir) {
        let mut count = 0usize;
        for entry in entries.flatten() {
            let run_dir = entry.path();
            if run_dir.is_dir() {
                watch_run_dir(&mut debouncer, &run_dir);
                count += 1;
            }
        }
        info!(
            "Watching {count} run dir(s) under: {}",
            runs_dir.display()
        );
    }

    Some(debouncer)
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

/// Detect if a changed path is a run-scoped pipeline file.
/// Expected patterns:
///   `<runs_dir>/<run-id>/pipeline.yaml` → kind "yaml"
///   `<runs_dir>/<run-id>/pipeline.prompts/<node-id>.md` → kind "prompt"
fn detect_run_scoped_change(
    path: &std::path::Path,
    runs_dir: &std::path::Path,
) -> Option<RunPipelineModified> {
    let relative = path.strip_prefix(runs_dir).ok()?;
    let mut components = relative.components();
    let run_id = components.next()?.as_os_str().to_str()?.to_string();
    let second = components.next()?.as_os_str().to_str()?;

    if second == "pipeline.yaml" && components.next().is_none() {
        return Some(RunPipelineModified {
            run_id,
            kind: "yaml",
            path: path.to_path_buf(),
        });
    }

    if second == "pipeline.prompts" {
        let file = components.next()?.as_os_str().to_str()?;
        if file.ends_with(".md") && components.next().is_none() {
            return Some(RunPipelineModified {
                run_id,
                kind: "prompt",
                path: path.to_path_buf(),
            });
        }
    }

    None
}

fn is_recent_self_write(map: &Mutex<HashMap<PathBuf, Instant>>, path: &std::path::Path) -> bool {
    let guard = match map.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    guard
        .get(path)
        .is_some_and(|t| t.elapsed() < SELF_WRITE_TTL)
}

fn seed_mtimes(map: &Mutex<HashMap<PathBuf, SystemTime>>, dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut guard = match map.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str());
        if path.is_file() && ext == Some("yaml") {
            if let Ok(t) = entry.metadata().and_then(|m| m.modified()) {
                guard.insert(path, t);
            }
        } else if path.is_dir()
            && path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".prompts"))
        {
            if let Ok(prompts) = std::fs::read_dir(&path) {
                for p in prompts.flatten() {
                    let pp = p.path();
                    if pp.extension().and_then(|e| e.to_str()) == Some("md") {
                        if let Ok(t) = p.metadata().and_then(|m| m.modified()) {
                            guard.insert(pp, t);
                        }
                    }
                }
            }
        }
    }
}

/// Seed mtimes for existing run-scoped pipeline files.
fn seed_run_mtimes(map: &Mutex<HashMap<PathBuf, SystemTime>>, runs_dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(runs_dir) else {
        return;
    };
    let mut guard = match map.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    for entry in entries.flatten() {
        let run_dir = entry.path();
        if !run_dir.is_dir() {
            continue;
        }
        let yaml_path = run_dir.join("pipeline.yaml");
        if yaml_path.is_file() {
            if let Ok(t) = std::fs::metadata(&yaml_path).and_then(|m| m.modified()) {
                guard.insert(yaml_path, t);
            }
        }
        let prompts_dir = run_dir.join("pipeline.prompts");
        if prompts_dir.is_dir() {
            if let Ok(prompts) = std::fs::read_dir(&prompts_dir) {
                for p in prompts.flatten() {
                    let pp = p.path();
                    if pp.extension().and_then(|e| e.to_str()) == Some("md") {
                        if let Ok(t) = p.metadata().and_then(|m| m.modified()) {
                            guard.insert(pp, t);
                        }
                    }
                }
            }
        }
    }
}

fn content_actually_changed(
    map: &Mutex<HashMap<PathBuf, SystemTime>>,
    path: &std::path::Path,
) -> bool {
    let current = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let mut guard = match map.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    match current {
        Some(now) => {
            let prev = guard.get(path).copied();
            if prev == Some(now) {
                false
            } else {
                guard.insert(path.to_path_buf(), now);
                true
            }
        }
        None => {
            guard.remove(path);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn detects_run_scoped_pipeline_yaml() {
        let runs = Path::new("/repo/.maestro/runs");
        let path = runs.join("run-123/pipeline.yaml");
        let result = detect_run_scoped_change(&path, runs).unwrap();
        assert_eq!(result.run_id, "run-123");
        assert_eq!(result.kind, "yaml");
    }

    #[test]
    fn detects_run_scoped_prompt_md() {
        let runs = Path::new("/repo/.maestro/runs");
        let path = runs.join("run-123/pipeline.prompts/planner.md");
        let result = detect_run_scoped_change(&path, runs).unwrap();
        assert_eq!(result.run_id, "run-123");
        assert_eq!(result.kind, "prompt");
    }

    #[test]
    fn ignores_unrelated_md_in_run_dir() {
        let runs = Path::new("/repo/.maestro/runs");
        let path = runs.join("run-123/README.md");
        assert!(detect_run_scoped_change(&path, runs).is_none());
    }

    #[test]
    fn ignores_md_in_run_worktree() {
        let runs = Path::new("/repo/.maestro/runs");
        let path = runs.join("run-123/worktree/docs/design-brief.md");
        assert!(detect_run_scoped_change(&path, runs).is_none());
    }

    #[test]
    fn ignores_artifact_md_in_run_worktree() {
        let runs = Path::new("/repo/.maestro/runs");
        let path = runs.join("run-123/worktree/.maestro/artifacts/planner/iter-1/plan.md");
        assert!(detect_run_scoped_change(&path, runs).is_none());
    }

    #[test]
    fn ignores_sandcastle_md_in_run_worktree() {
        let runs = Path::new("/repo/.maestro/runs");
        let path = runs.join("run-123/worktree/.sandcastle/implement-prompt.md");
        assert!(detect_run_scoped_change(&path, runs).is_none());
    }

    #[test]
    fn ignores_yaml_not_named_pipeline() {
        let runs = Path::new("/repo/.maestro/runs");
        let path = runs.join("run-123/config.yaml");
        assert!(detect_run_scoped_change(&path, runs).is_none());
    }

    #[test]
    fn ignores_path_outside_runs_dir() {
        let runs = Path::new("/repo/.maestro/runs");
        let path = Path::new("/repo/.maestro/pipelines/my-pipeline.yaml");
        assert!(detect_run_scoped_change(path, runs).is_none());
    }
}
