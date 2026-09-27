//! Archiving a Run (`cleanup_run`, a Pipeline Manager command): the Run and its
//! terminated descendants, grandchildren first, through one teardown
//! ([`teardown::archive_single_run`]). A transition the command triggers: the
//! command itself asked the Autorisation d'opération.

mod teardown;

use anyhow::Result;
use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tracing::error;

pub use teardown::archive_single_run;

use crate::application::events::load_projected;
use crate::domain::projection::project;
use crate::domain::state::{RunState, RunStatus};
use crate::Runs;

/// Every descendant of `run_id` (children, grandchildren, …) resolved through
/// the same mechanical `parent_run_id` link as `list_run_children`, in
/// **post-order**: a run always comes after all of its own descendants, so a
/// cascade that walks the list front to back tears grandchildren down before
/// their parent. Pure over the projected store; `run_id` itself is not listed.
pub fn descendant_runs_post_order(runs: &[RunState], run_id: &str) -> Vec<RunState> {
    let mut out = Vec::new();
    for child in runs {
        if child.parent_run_id.as_deref() != Some(run_id) {
            continue;
        }
        out.extend(descendant_runs_post_order(runs, &child.run_id));
        out.push(child.clone());
    }
    out
}

/// Project every run of the store (the read model behind `list_run_children`).
/// Unreadable or empty logs are skipped, never fatal: the cascade must not be
/// blocked by an unrelated corrupt run.
pub async fn load_all_projected_runs(state: &Runs) -> Result<Vec<RunState>> {
    let mut states = Vec::new();
    for id in state.store().run_ids().await? {
        let Ok(events) = state.store().events(&id).await else {
            continue;
        };
        if let Some(run_state) = project(&events) {
            states.push(run_state);
        }
    }
    Ok(states)
}

/// `POST /runs/{run_id}/commands {"kind":"cleanup_run"}` — archive a run **and
/// its terminated descendants** (#815).
///
/// The daemon already treats children as belonging to their parent (run tree,
/// `GET /runs/{id}/children`, `pdo complete` refused while children are in
/// flight), so archiving the root must reclaim the children's worktrees and
/// run dirs too, recursively, through the same teardown path
/// ([`archive_single_run`]). Descendants are archived grandchildren-first, the
/// parent last, so a failure mid-cascade never leaves an archived parent above
/// a still-live subtree.
///
/// Live descendants (running, awaiting-user, paused) are **never** archived by
/// the cascade: the request is refused with `409` naming them, and nothing is
/// touched. That is the conservative choice of the two the issue allowed
/// (refuse vs. archive the parent and orphan the live children): killing a
/// running child is not something an archive of its parent should do
/// implicitly, and orphaning it would silently break the tree the UI renders.
/// Stop the child first, then archive. Already-archived descendants are
/// skipped silently (idempotent): only the root itself being archived is a
/// `409`, as before.
pub async fn cleanup_run(state: &Runs, run_id: &str) -> Response {
    let _page_mount_guard = state.pages.hold().await;
    let (_, run_state) = match load_projected(state, run_id).await {
        Ok(t) => t,
        Err(resp) => return *resp,
    };

    if run_state.status == RunStatus::Archived {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "run is already archived" })),
        )
            .into_response();
    }

    let all_runs = match load_all_projected_runs(state).await {
        Ok(runs) => runs,
        Err(e) => {
            error!("cleanup_run: cannot list runs to resolve the children of {run_id}: {e:#}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("error: {e}") })),
            )
                .into_response();
        }
    };
    let descendants = descendant_runs_post_order(&all_runs, run_id);

    let live: Vec<&str> = descendants
        .iter()
        .filter(|d| d.status.is_live())
        .map(|d| d.run_id.as_str())
        .collect();
    if !live.is_empty() {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": format!(
                    "run has {} live child run(s); stop them before archiving the parent",
                    live.len()
                ),
                "live_children": live,
            })),
        )
            .into_response();
    }

    let mut archived_children: Vec<String> = Vec::new();
    for child in &descendants {
        if child.status == RunStatus::Archived {
            continue;
        }
        if let Err(resp) = archive_single_run(state, child).await {
            return *resp;
        }
        archived_children.push(child.run_id.clone());
    }

    if let Err(resp) = archive_single_run(state, &run_state).await {
        return *resp;
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "archived",
            "archived_children": archived_children,
        })),
    )
        .into_response()
}
