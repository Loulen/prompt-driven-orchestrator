use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::SELF_WRITE_TTL;

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
) -> Option<notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>> {
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

    let mut debouncer = match new_debouncer(
        Duration::from_secs(1),
        move |events: Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>| {
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
                if let Some(modified) = detect_run_scoped_change(path, &runs_dir_for_closure) {
                    info!(
                        "Run-scoped pipeline modified: run={} kind={} path={}",
                        modified.run_id,
                        modified.kind,
                        modified.path.display()
                    );
                    let _ = run_modified_tx.send(modified);
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
    ) {
        Ok(d) => d,
        Err(e) => {
            warn!("Failed to create file watcher: {e}");
            return None;
        }
    };

    if repo_pipelines_dir.exists() {
        if let Err(e) = debouncer
            .watcher()
            .watch(&repo_pipelines_dir, notify::RecursiveMode::Recursive)
        {
            warn!("Failed to watch repo pipelines dir: {e}");
        } else {
            info!("Watching repo pipelines: {}", repo_pipelines_dir.display());
        }
    } else {
        let _ = std::fs::create_dir_all(&repo_pipelines_dir);
        if let Err(e) = debouncer
            .watcher()
            .watch(&repo_pipelines_dir, notify::RecursiveMode::Recursive)
        {
            warn!("Failed to watch repo pipelines dir: {e}");
        }
    }

    if let Some(ref user_dir) = user_pipelines_dir {
        if user_dir.exists() {
            if let Err(e) = debouncer
                .watcher()
                .watch(user_dir, notify::RecursiveMode::Recursive)
            {
                warn!("Failed to watch user pipelines dir: {e}");
            } else {
                info!("Watching user pipelines: {}", user_dir.display());
            }
        }
    }

    // Watch runs directory for run-scoped pipeline edits
    if !runs_dir.exists() {
        let _ = std::fs::create_dir_all(&runs_dir);
    }
    if let Err(e) = debouncer
        .watcher()
        .watch(&runs_dir, notify::RecursiveMode::Recursive)
    {
        warn!("Failed to watch runs dir: {e}");
    } else {
        info!("Watching runs dir: {}", runs_dir.display());
    }

    Some(debouncer)
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
