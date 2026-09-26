//! The HTTP routes of the platform context. The composition root merges
//! [`router`] into the daemon's, behind the `identify` middleware that sets the
//! Actor of every request.
//!
//! ROUTER TRAP: a NEW top-level path prefix also needs its entry in the vite dev
//! proxy whitelist (`frontend/vite.config.ts`).

mod audit;
mod origin;
mod pages;
mod update;

use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use pdo_kernel::authorization::Denied;

pub use origin::OriginHint;

use crate::Platform;

/// The routes of the platform context, with their state.
pub fn router(platform: Arc<Platform>) -> Router {
    Router::new()
        .route("/pages", get(pages::list).post(pages::mount))
        .route("/pages/", get(pages::list))
        .route("/pages/{name}", get(pages::redirect).delete(pages::unmount))
        .route("/pages/{name}/", get(pages::serve_root))
        .route("/pages/{name}/{*path}", get(pages::serve_file))
        .route("/update", get(update::status))
        .route("/update/check", post(update::check))
        .route("/update/changelog", get(update::changelog))
        .route("/update/apply", post(update::apply))
        .route("/update/attempts/{id}/log", get(update::attempt_log))
        .route("/audit", get(audit::feed))
        .with_state(platform)
}

/// An Operation an extension refused: 403, the reason in the `error` the
/// gesture already shows.
fn refused(denied: Denied) -> Response {
    error(StatusCode::FORBIDDEN, denied.to_string())
}

/// A JSON error body, `{"error": …}`.
fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(serde_json::json!({ "error": message.into() }))).into_response()
}
