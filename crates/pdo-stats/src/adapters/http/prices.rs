//! `POST /settings/cost-prices/sync` (ADR-0034) — the ONLY route of the daemon
//! that reaches the network. It sits under `/settings` as a routing decision:
//! the price table is instance configuration, and `/stats` is a read-derivation
//! surface (ADR-0029).
//!
//! Codes: **409** a sync is already in flight · **502** source unreachable /
//! broken JSON / empty harvest, and **nothing is written** · **500** a disk
//! write failed · **200** otherwise, with `noop: true` + `reason` when the
//! table did not move. The 502 is deliberate: classing a network cut as a 500
//! would file it as a daemon defect.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;

use super::error;
use crate::application::prices::sync::SyncError;
use crate::Stats;

pub(super) async fn sync(
    State(stats): State<Arc<Stats>>,
    Extension(actor): Extension<Actor>,
) -> Response {
    match stats.sync_prices().execute(&actor).await {
        Ok(report) => Json(report).into_response(),
        Err(SyncError::Refused(denied)) => denied.into_response(),
        Err(SyncError::Failed(message)) => error(StatusCode::INTERNAL_SERVER_ERROR, message),
        Err(SyncError::InFlight) => {
            error(StatusCode::CONFLICT, "a price sync is already in flight")
        }
        Err(SyncError::Source(message)) => error(StatusCode::BAD_GATEWAY, message),
    }
}
