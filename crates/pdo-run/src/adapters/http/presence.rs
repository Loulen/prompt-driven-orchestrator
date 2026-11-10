//! `GET /runs/presences` (API 0.12): who has which Run open, for the Runs the
//! caller may read. What a view reads once, then follows on `/ws`
//! (`presence_arrived`, `presence_left`).

use std::sync::Arc;

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;
use serde_json::json;

use crate::Runs;

/// `{"runs": [{"run_id", "presences": [{"actor", "since"}…]}…]}`: every Run
/// someone has open that the caller may read, by Run id.
pub(super) async fn list(
    State(runs): State<Arc<Runs>>,
    Extension(actor): Extension<Actor>,
) -> Response {
    let open = runs.readable_presences(&actor).await;
    let runs: Vec<_> = open
        .into_iter()
        .map(|(run_id, presences)| json!({ "run_id": run_id, "presences": presences }))
        .collect();
    Json(json!({ "runs": runs })).into_response()
}
