//! The HTTP routes of the trigger context. The composition root merges
//! [`router`] into the daemon's, behind the `identify` middleware that sets the
//! Actor of every request.
//!
//! ROUTER TRAP: a NEW top-level path prefix also needs its entry in the vite dev
//! proxy whitelist (`frontend/vite.config.ts`).

mod control;
mod guard;
mod triggers;

use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

pub use guard::GuardTestResponse;

use crate::application::TriggerError;
use crate::Triggers;

/// The routes of the trigger context, with their state.
pub fn router(triggers: Arc<Triggers>) -> Router {
    Router::new()
        .route("/triggers", get(triggers::list).post(triggers::create))
        .route("/triggers/health", get(control::health))
        // Dry-run a guard command with no side effects — the opposite pole of "Run
        // now" (ADR-0027 addendum).
        .route("/triggers/guard/test", post(guard::test))
        // Global Trigger kill-switch.
        .route("/triggers/pause", post(control::pause))
        .route(
            "/triggers/{trigger_id}",
            get(triggers::get)
                .patch(triggers::update)
                .delete(triggers::delete),
        )
        .route("/triggers/{trigger_id}/fires", get(triggers::fires))
        // Manual "Run now" — a first-class fire.
        .route("/triggers/{trigger_id}/fire", post(control::fire))
        .with_state(triggers)
}

/// A use case's refusal, as the routes have always answered it: a refused
/// Operation is the kernel's `403`, everything else `{"error": …}` under its
/// status.
fn refusal(error: TriggerError) -> Response {
    let (status, message) = match error {
        TriggerError::Refused(denied) => return denied.into_response(),
        TriggerError::Invalid(m) => (StatusCode::BAD_REQUEST, m),
        TriggerError::NotFound(m) => (StatusCode::NOT_FOUND, m),
        TriggerError::Conflict(m) => (StatusCode::CONFLICT, m),
        TriggerError::Storage(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
    };
    (status, Json(serde_json::json!({ "error": message }))).into_response()
}
