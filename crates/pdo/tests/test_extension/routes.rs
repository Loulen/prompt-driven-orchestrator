//! The routes of the test extension, nested under `/ext/test/` by the core.

use pdo::extension_api::axum::http::header;
use pdo::extension_api::axum::response::IntoResponse;
use pdo::extension_api::axum::routing::{get, post};
use pdo::extension_api::axum::{Json, Router};
use serde_json::{json, Value};

use super::{Control, ID};

/// The UI bundle: a plain ES module (no build step) importing `react` as the
/// front shares it.
const BUNDLE: &str = include_str!("ui.js");

pub(super) fn router(control: Control) -> Router {
    let events = control.clone();
    let switch = control.clone();
    Router::new()
        .route(
            "/",
            get(move || {
                let data_dir = control.data_dir();
                async move {
                    Json(json!({
                        "id": ID,
                        "data_dir": data_dir.map(|dir| dir.display().to_string()),
                    }))
                }
            }),
        )
        .route(
            "/events",
            get(move || {
                let events = events.events();
                async move { Json(Value::Array(events)) }
            }),
        )
        .route(
            "/ui.js",
            get(|| async { ([(header::CONTENT_TYPE, "text/javascript")], BUNDLE).into_response() }),
        )
        .route(
            "/incompatible",
            post(move |Json(body): Json<Value>| {
                let enabled = body["enabled"].as_bool().unwrap_or(true);
                switch.set_incompatible(enabled);
                async move { Json(json!({ "incompatible": enabled })) }
            }),
        )
}
