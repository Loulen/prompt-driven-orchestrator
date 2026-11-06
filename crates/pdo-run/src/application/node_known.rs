//! Does a Run know a node? Asked before any gesture that targets one, so a
//! mistyped node id neither reopens a finished Run nor leaves a ghost node in
//! its state: the gesture answers `404` and nothing is written.

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::application::paths::{effective_repo_root, resolve_run_pipeline_path};
use crate::domain::state;
use crate::Runs;
use pdo_pipeline::domain::pipeline;

/// `Some(404)` when neither the Run's state nor its pipeline snapshot names
/// `node_id`. A snapshot that cannot be read or parsed answers `None`: the
/// gesture then gives its own answer rather than a blind refusal.
pub fn unknown_node(
    state: &Runs,
    run_id: &str,
    run_state: &state::RunState,
    node_id: &str,
) -> Option<Response> {
    if run_state.nodes.contains_key(node_id) {
        return None;
    }
    let repo_root = effective_repo_root(state, run_state);
    let pipeline_path = resolve_run_pipeline_path(&repo_root, run_id, &run_state.pipeline_name);
    let pipeline = std::fs::read_to_string(&pipeline_path)
        .ok()
        .and_then(|yaml| pipeline::parse_pipeline(&yaml).ok())?
        .pipeline;
    if pipeline.nodes.iter().any(|n| n.id == node_id) {
        return None;
    }
    Some(node_not_found(node_id))
}

/// The answer to a node the Run does not know, the same on every node
/// gesture (the body of the retry and restart verdicts' `node_not_found`).
pub fn node_not_found(node_id: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "error": "node_not_found",
            "node_id": node_id,
            "message": format!("node '{node_id}' not found in the run's pipeline"),
        })),
    )
        .into_response()
}
