//! `GET /stale/health` — the liveness sweep's heartbeat. On an idle-stall
//! recurrence this is what distinguishes a *dead sweep* (heartbeat frozen) from
//! a per-node probe miss (heartbeat advancing).

use std::sync::Arc;

use axum::extract::{Json, State};
use axum::response::{IntoResponse, Response};

use crate::application::stale::detector::STALE_TICK_INTERVAL_SECS;
use crate::Runs;

pub(super) async fn health(State(state): State<Arc<Runs>>) -> Response {
    let ms = state.last_stale_tick_ms();
    let last_tick_at = (ms != 0)
        .then(|| chrono::DateTime::from_timestamp_millis(ms))
        .flatten()
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
    Json(serde_json::json!({
        "last_tick_at": last_tick_at,
        "tick_interval_secs": STALE_TICK_INTERVAL_SECS,
        // Live nodes found stuck on Claude Code's usage-limit menu as of the last
        // sweep. Observability-only — the node stays Running.
        "blocked_on_limit": state.blocked_on_limit(),
    }))
    .into_response()
}
