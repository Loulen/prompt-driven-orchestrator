//! What a retry of a node would invalidate: the Retry button's confirmation.

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::application::events::load_events;
use crate::application::node_known::unknown_node;
use crate::application::paths::effective_repo_root;
use crate::application::paths::resolve_pipeline_path;
use crate::application::paths::run_scoped_pipeline_path;
use crate::domain::projection;
use crate::domain::{graph_resolver, loop_region};
use crate::Runs;
use pdo_pipeline::domain::pipeline;
use pdo_workspace::adapters::git::worktrees::worktree_dir_for_run;

/// What a retry of `node_id` would invalidate (a read: it asks no
/// authorization).
pub async fn retry_preview(state: &Runs, run_id: String, node_id: String) -> Response {
    let events = match load_events(state, &run_id).await {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };
    let run_state = match projection::project(&events) {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "run not found" })),
            )
                .into_response();
        }
    };
    if let Some(refusal) = unknown_node(state, &run_id, &run_state, &node_id) {
        return refusal;
    }

    let repo_root = effective_repo_root(state, &run_state);
    let pipeline_path = {
        let run_scoped = run_scoped_pipeline_path(&repo_root, &run_id);
        if run_scoped.exists() {
            run_scoped
        } else {
            resolve_pipeline_path(&repo_root, &run_state.pipeline_name)
        }
    };
    let Ok(yaml) = std::fs::read_to_string(&pipeline_path) else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "cannot read pipeline" })),
        )
            .into_response();
    };
    let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "cannot parse pipeline" })),
        )
            .into_response();
    };
    let pipeline_def = parse_result.pipeline;

    let worktree_dir = worktree_dir_for_run(&repo_root, &run_id);
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");

    // Must mirror `node_retry`'s loop-aware invalidation exactly, or the preview
    // promises something the retry will not do.
    let excluded_edges = loop_region::bounded_reentry_edges(&pipeline_def);
    let mut downstream: Vec<String> =
        graph_resolver::downstream_subgraph_excluding(&pipeline_def, &node_id, &excluded_edges)
            .into_iter()
            .collect();
    downstream.sort();

    let with_artifacts: Vec<&String> = downstream
        .iter()
        .filter(|nid| artifacts_dir.join(nid.as_str()).exists())
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "downstream": downstream,
            "affected_count": with_artifacts.len(),
            "with_artifacts": with_artifacts,
        })),
    )
        .into_response()
}
