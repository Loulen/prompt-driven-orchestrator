//! `POST /runs/{run_id}/nodes/{node_id}/wait-user` — the declared wait (#588 /
//! ADR-0069): the agent says it is waiting on its user.

use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;
use serde::Deserialize;

use super::refused;
use crate::application::declared_wait::{DeclareWaitCommand, DeclareWaitError};
use crate::domain::declared_wait::{DeclareOutcome, WaitRefusal};
use crate::Session;

#[derive(Deserialize, Default)]
pub(super) struct WaitUserRequest {
    #[serde(default)]
    iter: Option<i64>,
    #[serde(default)]
    message: Option<String>,
}

/// Accepted on any node holding a live session; named `409` on a `script` node
/// or without a live session; `noop` on a repeat with the same message.
pub(super) async fn declare(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
    AxumPath((run_id, node_id)): AxumPath<(String, String)>,
    body: Option<Json<WaitUserRequest>>,
) -> Response {
    let Json(body) = body.unwrap_or_default();
    let iter = body.iter.unwrap_or(1);
    let command = DeclareWaitCommand {
        run_id: run_id.clone(),
        node_id: node_id.clone(),
        iter,
        message: body.message,
    };
    match session.declare_wait().execute(&actor, command).await {
        Ok(outcome) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "awaiting": true,
                "noop": outcome == DeclareOutcome::Noop,
                "refreshed": outcome == DeclareOutcome::Refreshed,
            })),
        )
            .into_response(),
        Err(DeclareWaitError::Refused(denied)) => refused(denied),
        Err(DeclareWaitError::Wait(refusal)) => wait_refused(&refusal, &run_id, &node_id, iter),
    }
}

/// Every refusal is named; the CLI maps them to exit 3 (refused, still your
/// turn — nothing terminal happened).
fn wait_refused(refusal: &WaitRefusal, run_id: &str, node_id: &str, iter: i64) -> Response {
    let status = if refusal.is_not_found() {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::CONFLICT
    };
    (
        status,
        Json(serde_json::json!({
            "error": refusal.slug(),
            "recoverable": true,
            "message": refusal.message(run_id, node_id, iter),
        })),
    )
        .into_response()
}
