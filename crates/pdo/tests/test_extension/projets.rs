//! What the test extension reads of the Projets and signals of the Triggers
//! through its `Host` (API 0.11): the core's fixture for these seams.
//!
//! - `GET /ext/test/projects/<id>/name`: `{"name": <the name or null>}`, as
//!   `Host::project_name` answers it, synchronously, from a route;
//! - `POST /ext/test/triggers-changed`: `Host::triggers_changed`, as an
//!   extension does once a suspension's cause changed;
//! - `PUT /ext/test/notes/<id>` (`{"project": <id>, "title": …}`): the caller
//!   updates the [`NOTE`](super::NOTE) `<id>` of that Projet through the
//!   Host, which publishes `note_changed` guarded by the read of the note in
//!   its Projet, then the applied Operation: what an extension holding
//!   entities per Projet does, so that `/ws` sends them only to who may read
//!   them.

use pdo::extension_api::axum::extract::Path;
use pdo::extension_api::axum::http::StatusCode;
use pdo::extension_api::axum::response::{IntoResponse, Response};
use pdo::extension_api::axum::routing::{get, post, put};
use pdo::extension_api::axum::{Extension, Json, Router};
use pdo::extension_api::{
    Actor, EntityRef, ExtensionEvent, Operation, OperationApplied, ResourceKind, Verb,
};
use serde_json::{json, Value};

use super::creation::Creation;

/// The name of the event an update of a note publishes.
pub const NOTE_CHANGED: &str = "note_changed";

pub(super) fn routes(creation: Creation) -> Router {
    let signal = creation.clone();
    let notes = creation.clone();
    Router::new()
        .route(
            "/notes/{id}",
            put(
                move |Extension(actor): Extension<Actor>,
                      Path(id): Path<String>,
                      Json(body): Json<Value>| {
                    let answer = update_note(&notes, actor, &id, &body);
                    async move { answer }
                },
            ),
        )
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

/// Update the note `id` in the Projet `body.project` as `actor`: `200` and
/// the note, or the refusal.
fn update_note(creation: &Creation, actor: Actor, id: &str, body: &Value) -> Response {
    let Some(host) = creation.host() else {
        return not_started().into_response();
    };
    let kind = ResourceKind::extension(super::ID, super::NOTE);
    let projects: Vec<String> = body["project"].as_str().map(str::to_string).into_iter().collect();
    let update = Operation::new(kind.clone(), Verb::Update)
        .on(id)
        .in_projects(projects.clone());
    if let Err(denied) = host.authorize(&actor, &update).into_result() {
        return denied.into_response();
    }
    let note = json!({ "id": id, "project": body["project"], "title": body["title"] });
    let read = Operation::new(kind.clone(), Verb::Read)
        .on(id)
        .in_projects(projects);
    host.publish(ExtensionEvent::new(NOTE_CHANGED, note.clone()).guarded_by(read));
    host.applied(OperationApplied::new(
        actor,
        update,
        Some(EntityRef::new(kind, id)),
    ));
    (StatusCode::OK, Json(note)).into_response()
}

fn not_started() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "error": "the test extension has no Host yet" })),
    )
}
