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

/// The icon of its page and its command: a plain SVG drawn in `currentColor`.
const ICON: &str = include_str!("icon.svg");

pub(super) fn router(control: Control) -> Router {
    let events = control.clone();
    let operations = control.clone();
    let switch = control.clone();
    let commands = control.clone();
    let record = control.clone();
    let sections = control.clone();
    let saved = control.clone();
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
            "/operations",
            get(move || {
                let seen: Vec<Value> = operations
                    .seen()
                    .into_iter()
                    .map(|(actor, operation)| json!({ "actor": actor, "operation": operation }))
                    .collect();
                async move { Json(Value::Array(seen)) }
            }),
        )
        .route(
            "/ui.js",
            get(|| async { ([(header::CONTENT_TYPE, "text/javascript")], BUNDLE).into_response() }),
        )
        .route(
            "/icon.svg",
            get(|| async { ([(header::CONTENT_TYPE, "image/svg+xml")], ICON).into_response() }),
        )
        .route(
            "/commands",
            get(move || {
                let commands = commands.commands();
                async move { Json(Value::Array(commands)) }
            })
            .post(move |Json(context): Json<Value>| {
                record.record_command(context);
                async move { Json(json!({ "recorded": true })) }
            }),
        )
        .route(
            "/sections",
            get(move || {
                let sections = sections.sections();
                async move { Json(Value::Array(sections)) }
            })
            .post(move |Json(body): Json<Value>| {
                saved.record_section(body);
                async move { Json(json!({ "recorded": true })) }
            }),
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
