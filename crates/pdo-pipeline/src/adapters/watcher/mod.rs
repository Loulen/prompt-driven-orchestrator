//! The watcher of the instance registry and of the Runs' own pipelines: an
//! outside edit of a registry file is announced as `pipeline_changed`, marked
//! `outside`, an edit
//! of a Run's `pipeline.yaml` or prompt is handed to the Run.

mod debouncer;
mod mtimes;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use notify_debouncer_mini::{new_debouncer, new_debouncer_opt, DebouncedEventKind};
use tokio::sync::broadcast;
use tracing::{info, warn};

pub use debouncer::{watch_run_dir, PipelineDebouncer};
use debouncer::{DEBOUNCE, POLL_INTERVAL};
use mtimes::{content_actually_changed, seed_mtimes, seed_run_mtimes};

use super::self_writes::SelfWrites;

#[derive(Debug, Clone)]
pub struct RunPipelineModified {
    pub run_id: String,
    pub kind: &'static str, // "yaml" or "prompt"
    pub path: PathBuf,
}

pub fn spawn_watcher(
    repo_root: PathBuf,
    instance_pipelines_dir: Option<PathBuf>,
    event_tx: broadcast::Sender<serde_json::Value>,
    self_writes: SelfWrites,
    run_modified_tx: tokio::sync::mpsc::UnboundedSender<RunPipelineModified>,
) -> Option<PipelineDebouncer> {
    let runs_dir = repo_root.join(".pdo").join("runs");

    let tx = Arc::new(event_tx);
    let mtimes: Arc<Mutex<HashMap<PathBuf, SystemTime>>> = Arc::new(Mutex::new(HashMap::new()));
    if let Some(ref pipeline_dir) = instance_pipelines_dir {
        seed_mtimes(&mtimes, pipeline_dir);
    }
    seed_run_mtimes(&mtimes, &runs_dir);

    let runs_dir_for_closure = runs_dir.clone();
    let pipelines_dir_for_closure = instance_pipelines_dir.clone();

    // One shared handler, fed by both backends (a given path only ever
    // reports through the backend that registered it).
    type DebounceResult = Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>;
    let handler: Arc<dyn Fn(DebounceResult) + Send + Sync> =
        Arc::new(move |events: DebounceResult| {
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

                if self_writes.is_recent(path) {
                    info!(
                        "Pipeline file changed (self-write, suppressed): {}",
                        path.display()
                    );
                    continue;
                }

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
                if !pipelines_dir_for_closure
                    .as_deref()
                    .is_some_and(|dir| is_managed_pipeline_change(path, dir))
                {
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

                // `outside`: not one of the daemon's own writes, which it
                // announces itself (ADR-0088: kept as a version, then merged).
                let msg = serde_json::json!({
                    "type": "pipeline_changed",
                    "pipeline_id": pipeline_id,
                    "path": path.to_string_lossy(),
                    "outside": true,
                });

                let _ = tx.send(msg);
                info!("Pipeline file changed: {}", path.display());
            }
        });

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

    if let Some(ref pipeline_dir) = instance_pipelines_dir {
        if !pipeline_dir.exists() {
            let _ = std::fs::create_dir_all(pipeline_dir);
        }
        if let Err(e) = debouncer.watch(pipeline_dir, notify::RecursiveMode::Recursive) {
            warn!("Failed to watch instance pipelines dir: {e}");
        } else {
            info!("Watching instance pipelines: {}", pipeline_dir.display());
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
        info!("Watching {count} run dir(s) under: {}", runs_dir.display());
    }

    Some(debouncer)
}

pub(crate) fn is_managed_pipeline_change(path: &Path, instance_pipelines_dir: &Path) -> bool {
    path.starts_with(instance_pipelines_dir)
}

/// Expected patterns:
///   `<runs_dir>/<run-id>/pipeline.yaml` → kind "yaml"
///   `<runs_dir>/<run-id>/pipeline.prompts/<node-id>.md` → kind "prompt"
pub(crate) fn detect_run_scoped_change(
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn detects_run_scoped_pipeline_yaml() {
        let runs = Path::new("/repo/.pdo/runs");
        let path = runs.join("run-123/pipeline.yaml");
        let result = detect_run_scoped_change(&path, runs).unwrap();
        assert_eq!(result.run_id, "run-123");
        assert_eq!(result.kind, "yaml");
    }

    #[test]
    fn detects_run_scoped_prompt_md() {
        let runs = Path::new("/repo/.pdo/runs");
        let path = runs.join("run-123/pipeline.prompts/planner.md");
        let result = detect_run_scoped_change(&path, runs).unwrap();
        assert_eq!(result.run_id, "run-123");
        assert_eq!(result.kind, "prompt");
    }

    #[test]
    fn ignores_unrelated_md_in_run_dir() {
        let runs = Path::new("/repo/.pdo/runs");
        let path = runs.join("run-123/README.md");
        assert!(detect_run_scoped_change(&path, runs).is_none());
    }

    #[test]
    fn ignores_md_in_run_worktree() {
        let runs = Path::new("/repo/.pdo/runs");
        let path = runs.join("run-123/worktree/docs/design-brief.md");
        assert!(detect_run_scoped_change(&path, runs).is_none());
    }

    #[test]
    fn ignores_artifact_md_in_run_worktree() {
        let runs = Path::new("/repo/.pdo/runs");
        let path = runs.join("run-123/worktree/.pdo/artifacts/planner/iter-1/plan.md");
        assert!(detect_run_scoped_change(&path, runs).is_none());
    }

    #[test]
    fn ignores_sandcastle_md_in_run_worktree() {
        let runs = Path::new("/repo/.pdo/runs");
        let path = runs.join("run-123/worktree/.sandcastle/implement-prompt.md");
        assert!(detect_run_scoped_change(&path, runs).is_none());
    }

    #[test]
    fn ignores_yaml_not_named_pipeline() {
        let runs = Path::new("/repo/.pdo/runs");
        let path = runs.join("run-123/config.yaml");
        assert!(detect_run_scoped_change(&path, runs).is_none());
    }

    #[test]
    fn ignores_path_outside_runs_dir() {
        let runs = Path::new("/repo/.pdo/runs");
        let path = Path::new("/repo/.pdo/pipelines/my-pipeline.yaml");
        assert!(detect_run_scoped_change(path, runs).is_none());
    }

    #[test]
    fn only_instance_registry_paths_are_pipeline_changes() {
        let instance = Path::new("/home/user/.pdo/pipelines");

        assert!(is_managed_pipeline_change(
            Path::new("/home/user/.pdo/pipelines/managed.yaml"),
            instance
        ));
        assert!(!is_managed_pipeline_change(
            Path::new("/repo/.pdo/pipelines/legacy.yaml"),
            instance
        ));
    }
}
