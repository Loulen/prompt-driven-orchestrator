//! `GET /runs/reapable`: the terminal Runs whose worktrees are still on disk,
//! so a human or a janitor pipeline can reclaim it through `cleanup_run`.

use std::path::Path;

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::application::paths::effective_repo_root;
use crate::domain::projection::project;
use crate::domain::state::RunStatus;
use crate::Runs;

/// One entry of `GET /runs/reapable`: a terminal, non-archived run whose on-disk
/// worktree(s) still exist and can be reclaimed via the explicit `cleanup_run`
/// command.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct ReapableEntry {
    pub run_id: String,
    pub pipeline_name: String,
    /// The run's terminal status. Deliberately NOT pre-filtered — the consumer
    /// applies its own status policy.
    pub status: RunStatus,
    /// ISO timestamp of the terminal transition — the age a janitor TTL policy
    /// compares against.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub completed_at: Option<String>,
    /// Seconds since `completed_at` (`None` when it is absent/unparseable), so a
    /// consumer can apply an age threshold without re-parsing timestamps.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub age_secs: Option<i64>,
    /// Always `true` on an emitted entry (the endpoint only lists worktree-present
    /// runs); carried explicitly so the payload is self-describing.
    pub worktree_present: bool,
    /// Resolved target repo the worktree lives under (`effective_repo_root`).
    pub effective_repo: String,
    /// Best-effort recursive byte size of the run dir, present only when the
    /// caller passes `?size=true` (the default omits it to stay fast). `None`
    /// when not requested or when sizing failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub approx_disk_bytes: Option<u64>,
}

#[derive(Deserialize)]
pub struct ReapableQuery {
    /// `?size=true` computes `approx_disk_bytes` per run (walks the tree, so it
    /// is opt-in). Defaults to `false` to keep the listing cheap.
    #[serde(default)]
    pub size: bool,
}

/// Best-effort recursive byte size of a directory tree (apparent size). Symlinks are
/// not followed, so it cannot cycle. `None` only when the root does not exist;
/// per-entry I/O errors are skipped. Opt-in behind `?size=true` because a run
/// worktree can hold a full `node_modules`.
pub fn dir_size_bytes(root: &Path) -> Option<u64> {
    if !root.exists() {
        return None;
    }
    let mut total: u64 = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if ft.is_symlink() {
                continue; // don't follow — avoids cycles and double counting
            }
            if ft.is_dir() {
                stack.push(entry.path());
            } else if ft.is_file() {
                if let Ok(meta) = entry.metadata() {
                    total = total.saturating_add(meta.len());
                }
            }
        }
    }
    Some(total)
}

impl Runs {
/// GET /runs/reapable — read-only surfacing of terminal runs whose on-disk
/// worktree(s) still exist, so a human or a janitor pipeline can reclaim the disk via
/// the explicit `cleanup_run` command.
///
/// DOCTRINE (ADR-0012a): this endpoint NEVER deletes anything. It only *surfaces*
/// candidates; the irreversible teardown stays a pipeline/human action.
///
/// A run is emitted iff it is terminal AND not already `Archived` AND a `worktree/`
/// or `nodes/` dir still exists under its run dir.
    pub async fn list_reapable(&self, q: ReapableQuery) -> Response {
    let state = self;
    let run_ids = match state.store().run_ids().await {
        Ok(ids) => ids,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };

    let now = chrono::Utc::now();
    let mut entries = Vec::new();
    for run_id in run_ids {
        let events = match state.store().events(&run_id).await {
            Ok(e) => e,
            Err(_) => continue,
        };
        let Some(run_state) = project(&events) else {
            continue;
        };

        // `Archived` is terminal too, so `is_terminal()` alone is not enough.
        let eligible =
            run_state.status.is_terminal() && run_state.status != RunStatus::Archived;
        if !eligible {
            continue;
        }

        let repo_root = effective_repo_root(state, &run_state);
        let run_dir = repo_root.join(".pdo").join("runs").join(&run_id);
        let worktree_present = run_dir.join("worktree").exists() || run_dir.join("nodes").exists();
        if !worktree_present {
            continue;
        }

        // `completed_at` is borrowed here and moved into the entry below.
        let age_secs = run_state
            .completed_at
            .as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| {
                now.signed_duration_since(dt.with_timezone(&chrono::Utc))
                    .num_seconds()
            });

        let approx_disk_bytes = if q.size {
            dir_size_bytes(&run_dir)
        } else {
            None
        };

        entries.push(ReapableEntry {
            run_id: run_state.run_id,
            pipeline_name: run_state.pipeline_name,
            status: run_state.status,
            completed_at: run_state.completed_at,
            age_secs,
            worktree_present,
            effective_repo: repo_root.to_string_lossy().into_owned(),
            approx_disk_bytes,
        });
    }

    Json(entries).into_response()
}
}
