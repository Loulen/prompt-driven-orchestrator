//! `ReleaseCompletion`: releasing the completion an interactive node waits on,
//! the gesture of the operator (the node's button or the Manager's command).

use std::sync::Arc;

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use pdo_kernel::Actor;

use crate::application::events::append_event;
use crate::application::events::load_events;
use crate::application::operations::on_node;
use crate::domain::{event, projection};
use crate::Runs;

pub fn release_refusal(slug: &str, message: impl Into<String>) -> Response {
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({
            "error": slug,
            "recoverable": false,
            "message": message.into(),
        })),
    )
        .into_response()
}

pub async fn release_node_completion(
    state: &Arc<Runs>,
    run_id: &str,
    node_id: &str,
    iter: i64,
) -> Response {
    let events = match load_events(state, run_id).await {
        Ok(events) => events,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("error: {e}") })),
            )
                .into_response();
        }
    };
    let Some(run_state) = projection::project(&events) else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "run_not_found", "recoverable": false })),
        )
            .into_response();
    };
    if !run_state.status.is_live() {
        return release_refusal(
            "run_not_live",
            format!("run {run_id} has no live session to release"),
        );
    }
    let Some(node) = run_state.nodes.get(node_id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "node_not_found", "recoverable": false })),
        )
            .into_response();
    };
    let Some(attempt) = node.iterations.iter().find(|attempt| attempt.iter == iter) else {
        return release_refusal(
            "node_session_not_live",
            format!("node {node_id} iter {iter} has no live session"),
        );
    };
    if !attempt.interactive {
        return release_refusal(
            "node_not_interactive",
            format!("node {node_id} is not interactive"),
        );
    }
    if !attempt.status.holds_session() {
        return release_refusal(
            "node_session_not_live",
            format!("node {node_id} iter {iter} has no live session"),
        );
    }
    if attempt.completion_released {
        return (
            StatusCode::OK,
            Json(serde_json::json!({ "ok": true, "released": true, "noop": true })),
        )
            .into_response();
    }

    let event = event::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::NodeCompletionReleased,
        node_id: Some(node_id.to_string()),
        iter: Some(iter),
        payload: None,
    };
    if let Err(e) = append_event(state, &event).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("error: {e}") })),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({ "ok": true, "released": true })),
    )
        .into_response()
}

/// `ReleaseCompletion`: the operator releases the completion an interactive
/// node's agent is waiting on (#588).
#[derive(Debug, Clone)]
pub struct ReleaseCompletion {
    pub run_id: String,
    pub node_id: String,
    pub iter: i64,
}

impl Runs {
    pub async fn release_completion(
        self: &Arc<Self>,
        actor: &Actor,
        command: ReleaseCompletion,
    ) -> Response {
        let ReleaseCompletion {
            run_id,
            node_id,
            iter,
        } = command;
        if let Err(denied) =
            self.authorize(actor, &on_node("release_completion", &run_id, &node_id))
        {
            return denied.into_response();
        }
        release_node_completion(self, &run_id, &node_id, iter).await
    }
}
