//! `POST /runs/{run_id}/nodes/{node_id}/wait-user` — the declared wait (#588 /
//! ADR-0069): the agent says it is waiting on its user; and
//! `POST /sessions/{session_id}/wait-user`, the same from the agent of a
//! Session hébergée (ADR-0091).

use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;
use serde::Deserialize;

use crate::application::declared_wait::{DeclareWaitCommand, DeclareWaitError};
use crate::application::hosted::HostedWaitError;
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
        Err(DeclareWaitError::Refused(denied)) => denied.into_response(),
        Err(DeclareWaitError::Wait(refusal)) => wait_refused(&refusal, &run_id, &node_id, iter),
    }
}

/// The body of a hosted session's `wait-user`.
#[derive(Deserialize, Default)]
pub(super) struct HostedWaitRequest {
    #[serde(default)]
    message: Option<String>,
}

/// Accepted on a Session hébergée whose agent runs; named `404` for a name
/// not recorded, `409` for a session not running; `noop` on a repeat with
/// the same message.
pub(super) async fn declare_hosted(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
    AxumPath(session_id): AxumPath<String>,
    body: Option<Json<HostedWaitRequest>>,
) -> Response {
    let Json(body) = body.unwrap_or_default();
    let hosted = session.hosted().clone();
    let id = session_id.clone();
    let declared = tokio::task::spawn_blocking(move || {
        hosted.declare_wait(&actor, &id, body.message.as_deref())
    })
    .await;
    match declared {
        Ok(Ok(outcome)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "awaiting": true,
                "noop": outcome == DeclareOutcome::Noop,
                "refreshed": outcome == DeclareOutcome::Refreshed,
            })),
        )
            .into_response(),
        Ok(Err(HostedWaitError::Refused(denied))) => denied.into_response(),
        Ok(Err(refusal)) => {
            let status = match refusal {
                HostedWaitError::NotFound => StatusCode::NOT_FOUND,
                _ => StatusCode::CONFLICT,
            };
            (
                status,
                Json(serde_json::json!({
                    "error": refusal.slug(),
                    "recoverable": true,
                    "message": refusal.message(&session_id),
                })),
            )
                .into_response()
        }
        Err(e) => super::error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
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
