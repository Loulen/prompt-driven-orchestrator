//! What the test extension reads of the Projets and signals of the Triggers
//! through its `Host` (API 0.11): the core's fixture for these seams.
//!
//! - `GET /ext/test/projects/<id>/name`: `{"name": <the name or null>}`, as
//!   `Host::project_name` answers it, synchronously, from a route;
//! - `POST /ext/test/triggers-changed`: `Host::triggers_changed`, as an
//!   extension does once a suspension's cause changed.

use pdo::extension_api::axum::extract::Path;
use pdo::extension_api::axum::http::StatusCode;
use pdo::extension_api::axum::routing::{get, post};
use pdo::extension_api::axum::{Json, Router};
use serde_json::json;

use super::creation::Creation;

pub(super) fn routes(creation: Creation) -> Router {
    let signal = creation.clone();
    Router::new()
        .route(
            "/projects/{id}/name",
            get(move |Path(id): Path<String>| {
                let answer = match creation.host() {
                    Some(host) => (
                        StatusCode::OK,
                        Json(json!({ "name": host.project_name(&id) })),
                    ),
                    None => not_started(),
                };
                async move { answer }
            }),
        )
        .route(
            "/triggers-changed",
            post(move || {
                let answer = match signal.host() {
                    Some(host) => {
                        host.triggers_changed();
                        (StatusCode::OK, Json(json!({ "signalled": true })))
                    }
                    None => not_started(),
                };
                async move { answer }
            }),
        )
}

fn not_started() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "error": "the test extension has no Host yet" })),
    )
}
