//! The HTTP routes of the stats context. The composition root merges
//! [`router`] into the daemon's, behind the `identify` middleware that sets the
//! Actor of every request.
//!
//! ROUTER TRAP: a NEW top-level path prefix also needs its entry in the vite dev
//! proxy whitelist (`frontend/vite.config.ts`).

mod absorptions;
mod prices;
mod sections;

use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};

use crate::Stats;

/// The routes of the stats context, with their state.
pub fn router(stats: Arc<Stats>) -> Router {
    Router::new()
        // Instance stats cockpit (ADR-0029). `overview` is cheap indexed SQL;
        // `cost` fans the memoized per-run cost over the visible period, so it
        // is fetched lazily, only when the cost tab is shown.
        .route("/stats/overview", get(sections::overview))
        .route("/stats/cost", get(sections::cost))
        .route("/stats/performance", get(sections::performance))
        // Absorptions (#888, ADR-0077): instance configuration, applied on
        // read by the three folds above.
        .route(
            "/stats/absorptions",
            get(absorptions::list).post(absorptions::combine),
        )
        .route("/stats/absorptions/member", delete(absorptions::uncombine))
        // Price sync (ADR-0034) — the only route that reaches the network.
        .route("/settings/cost-prices/sync", post(prices::sync))
        .with_state(stats)
}

/// A JSON error body, `{"error": …}`.
fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(serde_json::json!({ "error": message.into() }))).into_response()
}
