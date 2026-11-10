//! The heartbeat's controls: the global pause, its health, and "Run now".

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;
use pdo_platform::adapters::http::OriginHint;
use serde::Deserialize;

use super::refusal;
use crate::Triggers;

#[derive(Deserialize)]
pub(super) struct PauseTriggersRequest {
    paused: bool,
}

/// `POST /triggers/pause` — flip the global Trigger kill-switch.
pub(super) async fn pause(
    State(triggers): State<Arc<Triggers>>,
    Extension(actor): Extension<Actor>,
    OriginHint(hint): OriginHint,
    Json(req): Json<PauseTriggersRequest>,
) -> Response {
    match triggers.pause(&actor, hint, req.paused).await {
        Ok(()) => Json(serde_json::json!({ "ok": true, "paused": req.paused })).into_response(),
        Err(e) => refusal(e),
    }
}

/// `GET /triggers/health` — the heartbeat's liveness.
pub(super) async fn health(State(triggers): State<Arc<Triggers>>) -> Response {
    let health = triggers.health().await;
    Json(serde_json::json!({
        "last_tick_at": health.last_tick_at,
        "tick_interval_secs": health.tick_interval_secs,
        "paused": health.paused,
    }))
    .into_response()
}

/// `POST /triggers/{id}/fire` — a manual "Run now" (ADR-0027). A guard/overlap
/// skip is an honest `200 {fired: false}`, not an error.
pub(super) async fn fire(
    State(triggers): State<Arc<Triggers>>,
    Path(trigger_id): Path<String>,
    Extension(actor): Extension<Actor>,
) -> Response {
    match triggers.run_now(&actor, &trigger_id).await {
        Ok(Some(r)) if r.outcome == "fired" => Json(serde_json::json!({
            "ok": true,
            "fired": true,
            "run_id": r.run_id,
        }))
        .into_response(),
        Ok(Some(r)) => Json(serde_json::json!({
            "ok": true,
            "fired": false,
            "outcome": r.outcome,
            "reason": r.reason,
        }))
        .into_response(),
        // Unreachable today (due is forced, enabled was checked); stay truthful
        // rather than panic if the decision core evolves.
        Ok(None) => Json(serde_json::json!({
            "ok": true,
            "fired": false,
            "outcome": "skipped",
            "reason": "no action taken",
        }))
        .into_response(),
        Err(e) => refusal(e),
    }
}
