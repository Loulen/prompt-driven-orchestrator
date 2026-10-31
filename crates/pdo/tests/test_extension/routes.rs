//! The routes of the test extension, nested under `/ext/test/` by the core.

use pdo::extension_api::axum::extract::Path;
use pdo::extension_api::axum::http::{header, StatusCode};
use pdo::extension_api::axum::response::IntoResponse;
use pdo::extension_api::axum::routing::{delete, get, post};
use pdo::extension_api::axum::{Json, Router};
use serde_json::{json, Value};

use super::gate::{self, Gate};
use super::{Control, Refusal, ID};

/// The UI bundle: a plain ES module (no build step) importing `react` as the
/// front shares it.
const BUNDLE: &str = include_str!("ui.js");

/// The icon of its page and its command: a plain SVG drawn in `currentColor`.
const ICON: &str = include_str!("icon.svg");

/// The icon of its messages panel (API 0.20), drawn the same way.
const MESSAGES_ICON: &str = include_str!("messages.svg");

pub(super) fn router(control: Control) -> Router {
    let events = control.clone();
    let operations = control.clone();
    let refusal = control.clone();
    let set_refusal = control.clone();
    let switch = control.clone();
    let commands = control.clone();
    let record = control.clone();
    let sections = control.clone();
    let saved = control.clone();
    let gate_read = control.clone();
    let gate_write = control.clone();
    let launch = control.launch.clone();
    Router::new()
        .merge(launch_routes(launch, control.creation.clone()))
        .merge(super::creation::routes(control.creation.clone()))
        .merge(super::projets::routes(control.creation.clone()))
        .merge(super::run_view::routes(control.creation.clone()))
        .merge(super::sessions::routes(control.creation.clone()))
        .merge(super::messages::routes(control.creation.clone()))
        .merge(super::palette::routes(
            control.inbox.clone(),
            control.creation.clone(),
        ))
        // A sub-router with a fallback of its own: what the admission must
        // guard as well as a route (`/ext/test/nested/<anything>`).
        .nest(
            "/nested",
            Router::new().fallback(|| async { "the nested fallback" }),
        )
        .route(
            "/gate",
            get(move || {
                let gate = gate_read.gate();
                async move { Json(json!(gate)) }
            })
            .put(move |Json(gate): Json<Gate>| {
                gate_write.set_gate(gate.clone());
                async move { Json(json!(gate)) }
            }),
        )
        .route(
            "/sign-in",
            get(|| async { ([(header::CONTENT_TYPE, "text/html")], gate::SIGN_IN_PAGE) }),
        )
        .route(
            "/avatar.svg",
            get(|| async { ([(header::CONTENT_TYPE, "image/svg+xml")], gate::AVATAR_SVG) }),
        )
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
            "/refusal",
            get(move || {
                let refusal = refusal.refusal();
                async move { Json(json!({ "refusing": format!("{refusal:?}") })) }
            })
            .post(move |Json(body): Json<Value>| {
                let answer = match body["refuse"].as_str().map(Refusal::parse) {
                    Some(Ok(parsed)) => {
                        let text = format!("{parsed:?}");
                        set_refusal.set(parsed);
                        (StatusCode::OK, Json(json!({ "refusing": text })))
                    }
                    Some(Err(message)) => {
                        (StatusCode::BAD_REQUEST, Json(json!({ "error": message })))
                    }
                    None => (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": "expected {\"refuse\": \"<refusal>\"}" })),
                    ),
                };
                async move { answer }
            }),
        )
        .route(
            "/operations",
            get(move || {
                let seen: Vec<Value> = operations
                    .asked()
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
            "/messages.svg",
            get(|| async {
                ([(header::CONTENT_TYPE, "image/svg+xml")], MESSAGES_ICON).into_response()
            }),
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

/// API 0.8: what the extension gives a Node's launch, and its suspensions.
/// A suspension set or lifted is signalled to the core (API 0.11), as an
/// extension whose cause changed outside a Trigger's mutation does.
fn launch_routes(launch: super::launch::Launch, creation: super::creation::Creation) -> Router {
    let (read, configure, list, suspend, lift) = (
        launch.clone(),
        launch.clone(),
        launch.clone(),
        launch.clone(),
        launch,
    );
    let (on_suspend, on_lift) = (creation.clone(), creation);
    Router::new()
        .route(
            "/launch",
            get(move || {
                let state = read.state();
                async move { Json(state) }
            })
            .put(move |Json(body): Json<Value>| {
                configure.configure(&body);
                let state = configure.state();
                async move { Json(state) }
            }),
        )
        .route(
            "/suspensions",
            get(move || {
                let all = list.suspensions();
                async move { Json(all) }
            })
            .put(move |Json(body): Json<Value>| {
                let id = body["trigger_id"].as_str().unwrap_or_default().to_string();
                let reason = body["reason"]
                    .as_str()
                    .unwrap_or("suspended by the test extension");
                suspend.suspend(&id, reason);
                if let Some(host) = on_suspend.host() {
                    host.triggers_changed();
                }
                let all = suspend.suspensions();
                async move { Json(all) }
            }),
        )
        .route(
            "/suspensions/{id}",
            delete(move |Path(id): Path<String>| {
                let lifted = lift.lift(&id);
                if let Some(host) = on_lift.host() {
                    host.triggers_changed();
                }
                async move { Json(json!({ "lifted": lifted })) }
            }),
        )
}
