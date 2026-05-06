use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use tokio::sync::broadcast;
use tracing::{info, warn};

pub fn spawn_watcher(
    repo_root: PathBuf,
    event_tx: broadcast::Sender<serde_json::Value>,
) -> Option<notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>> {
    let repo_pipelines_dir = repo_root.join(".maestro").join("pipelines");
    let user_pipelines_dir = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|h| h.join(".maestro").join("pipelines"));

    let tx = Arc::new(event_tx);
    let repo_dir = repo_pipelines_dir.clone();
    let user_dir = user_pipelines_dir.clone();

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
        let _ = std::fs::create_dir_all(&repo_dir);
        if let Err(e) = debouncer
            .watcher()
            .watch(&repo_dir, notify::RecursiveMode::Recursive)
        {
            warn!("Failed to watch repo pipelines dir: {e}");
        }
    }

    if let Some(ref user_dir) = user_dir {
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
