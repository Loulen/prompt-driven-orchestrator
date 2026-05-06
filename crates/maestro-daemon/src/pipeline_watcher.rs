use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::SELF_WRITE_TTL;

pub fn spawn_watcher(
    repo_root: PathBuf,
    event_tx: broadcast::Sender<serde_json::Value>,
    recent_writes: Arc<Mutex<HashMap<PathBuf, Instant>>>,
) -> Option<notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>> {
    let repo_pipelines_dir = repo_root.join(".maestro").join("pipelines");
    let user_pipelines_dir = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|h| h.join(".maestro").join("pipelines"));

    let tx = Arc::new(event_tx);
    // Last observed content-mtime per path. Used to filter out spurious events
    // notify reports for read-only opens (we observed the watcher firing on
    // plain `read_to_string` calls — see Bug F notes in #17). Tracking mtime
    // gives a content-based answer regardless of which inotify mask the
    // platform actually surfaces.
    let mtimes: Arc<Mutex<HashMap<PathBuf, SystemTime>>> = Arc::new(Mutex::new(HashMap::new()));
    // Pre-populate from the dirs we're about to watch so the very first event
    // for a known file can compare against a real value (otherwise we'd treat
    // the platform's initial debounced batch as a content change).
    seed_mtimes(&mtimes, &repo_pipelines_dir);
    if let Some(ref user_dir) = user_pipelines_dir {
        seed_mtimes(&mtimes, user_dir);
    }

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

    Some(debouncer)
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

/// Walks `dir` (yaml at the top level, md inside `*.prompts/` subdirs) and
/// records the current mtime of each file we care about. Run once at startup
/// so `content_actually_changed` has a reference point for files that already
/// exist when the daemon boots.
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

/// Compares the file's current modified-time against the last value we saw and
/// records the new one. Returns true only if the mtime advanced — events without
/// a real content change (read-only opens, atime-only updates) are filtered
/// out by this check. If the file is missing (deletion), returns true so the
/// caller still emits a `pipeline_changed` and the frontend can refetch.
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
            // File no longer there — let the frontend learn about it.
            guard.remove(path);
            true
        }
    }
}
