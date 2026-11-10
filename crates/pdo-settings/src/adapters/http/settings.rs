//! `GET /settings` and `PUT /settings`: the instance settings page.

use std::sync::Arc;

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;

use super::refusal;
use crate::Settings;

/// The instance-wide config, per knob, as `{effective, source, stored, env,
/// default}` (ADR-0015). `GET /sessions` stays the lean status-bar view.
pub(super) async fn get(State(settings): State<Arc<Settings>>) -> Response {
    match settings.view().await {
        Ok(view) => Json(view).into_response(),
        Err(e) => refusal(e),
    }
}

/// The body is taken as raw JSON: a retired field must be refused naming
/// itself, and a type error must be a `400` with a message rather than axum's
/// opaque `422`.
pub(super) async fn put(
    State(settings): State<Arc<Settings>>,
    Extension(actor): Extension<Actor>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    match settings.update(&actor, body).await {
        Ok(view) => Json(view).into_response(),
        Err(e) => refusal(e),
    }
}
