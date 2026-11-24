//! The HTTP routes of the platform context. The composition root merges
//! [`router`] into the daemon's, behind the `identify` middleware that sets the
//! Actor of every request.
//!
//! ROUTER TRAP: a NEW top-level path prefix also needs its entry in the vite dev
//! proxy whitelist (`frontend/vite.config.ts`).

mod audit;
mod bug_report;
mod origin;
mod pages;
mod update;

use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};

pub use bug_report::{
    BugReportDestinationView, BugReportDestinations, BugReportScreenshot,
    RememberBugReportDestination, SendBugReport,
};
pub use origin::OriginHint;

use crate::Platform;

/// The largest body of `POST /bug-report`.
const BUG_REPORT_BODY_LIMIT: usize = 16 * 1024 * 1024;

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
        .route("/bug-report/destinations", get(bug_report::destinations))
        .route(
            "/bug-report",
            // A screenshot of 10 MB, in base64, in its JSON.
            post(bug_report::send).route_layer(DefaultBodyLimit::max(BUG_REPORT_BODY_LIMIT)),
        )
        .route("/bug-report/last", put(bug_report::remember))
        .with_state(platform)
}

/// A JSON error body, `{"error": …}`.
fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(serde_json::json!({ "error": message.into() }))).into_response()
}
