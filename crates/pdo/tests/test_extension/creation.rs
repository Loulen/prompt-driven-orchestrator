//! What the test extension does with the creation of a Run (API 0.9): the
//! core's fixture for the extension data of `POST /runs` and for
//! `Host::create_run`.
//!
//! - **`validate_create`**: refuses a value holding a string that starts with
//!   [`REFUSED`] (`{"cle": "refuse-me"}`, or `"refuse"` itself), with a reason
//!   naming it; accepts anything else.
//! - **`on_create_with_data`**: records each creation it stamps
//!   ([`Creation::creations`], `GET /ext/test/creations`) and stamps, beside
//!   `started_by`, `data.<key>` for each key of an object value (`test/data.cle`).
//! - The **timeline** ([`Creation::timeline`], in `GET /ext/test/creations`):
//!   `on_create <run>` and `node_env <run>/<node>` in the order the core called
//!   them, which proves a Run is stamped before its first Node.
//! - `POST /ext/test/runs` creates a Run through the extension's `Host`, on
//!   behalf of the caller (`{pipeline, input, repo, source_branch?, name?,
//!   extensions?}`), and answers what the Host answered.

use std::sync::{Arc, Mutex};

use pdo::extension_api::axum::http::StatusCode;
use pdo::extension_api::axum::routing::{get, post};
use pdo::extension_api::axum::{Extension, Json, Router};
use pdo::extension_api::{
    Actor, Attributes, EntityRef, Host, NewRun, NodeLaunch, ResourceKind, RunRepo,
};
use serde_json::{json, Value};

/// A string value starting with this is refused.
pub const REFUSED: &str = "refuse";

/// The creation side of the test extension's control.
#[derive(Clone, Default)]
pub struct Creation {
    creations: Arc<Mutex<Vec<Value>>>,
    timeline: Arc<Mutex<Vec<String>>>,
    host: Arc<Mutex<Option<Arc<dyn Host>>>>,
}

impl Creation {
    /// Every creation stamped, in order: `{"kind", "id", "actor", "data"}`.
    pub fn creations(&self) -> Vec<Value> {
        self.creations.lock().unwrap().clone()
    }

    /// `on_create <id>` and `node_env <run>/<node>`, in the order called.
    pub fn timeline(&self) -> Vec<String> {
        self.timeline.lock().unwrap().clone()
    }

    pub(super) fn keep_host(&self, host: Arc<dyn Host>) {
        *self.host.lock().unwrap() = Some(host);
    }

    pub(super) fn launched(&self, launch: &NodeLaunch) {
        self.timeline
            .lock()
            .unwrap()
            .push(format!("node_env {}/{}", launch.run_id, launch.node_id));
    }

    /// `validate_create`: the first refused string, with its key.
    pub(super) fn validate(&self, value: &Value) -> Result<(), String> {
        match refused(value, None) {
            Some((Some(key), text)) => Err(format!("the test extension refuses {key}={text}")),
            Some((None, text)) => Err(format!("the test extension refuses {text}")),
            None => Ok(()),
        }
    }

    /// `on_create_with_data`: record the creation; the `data.<key>`
    /// attributes of an object value.
    pub(super) fn stamped(
        &self,
        actor: &Actor,
        entity: &EntityRef,
        data: Option<&Value>,
    ) -> Attributes {
        self.timeline
            .lock()
            .unwrap()
            .push(format!("on_create {}", entity.id));
        self.creations.lock().unwrap().push(json!({
            "kind": entity.kind,
            "id": entity.id,
            "actor": actor,
            "data": data,
        }));
        match data.and_then(Value::as_object) {
            Some(object) => object
                .iter()
                .map(|(key, value)| {
                    let text = value
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| value.to_string());
                    (format!("data.{key}"), text)
                })
                .collect(),
            None => Attributes::new(),
        }
    }
}

/// The first string starting with [`REFUSED`] in `value`, with its key.
fn refused(value: &Value, key: Option<&str>) -> Option<(Option<String>, String)> {
    match value {
        Value::String(text) if text.starts_with(REFUSED) => {
            Some((key.map(str::to_string), text.clone()))
        }
        Value::Object(object) => object
            .iter()
            .find_map(|(key, value)| refused(value, Some(key))),
        Value::Array(items) => items.iter().find_map(|item| refused(item, key)),
        _ => None,
    }
}

/// `GET /creations`, `POST /runs`.
pub(super) fn routes(creation: Creation) -> Router {
    let read = creation.clone();
    Router::new()
        .route(
            "/creations",
            get(move || {
                let answer = json!({
                    "creations": read.creations(),
                    "timeline": read.timeline(),
                });
                async move { Json(answer) }
            }),
        )
        .route(
            "/runs",
            post(
                move |Extension(actor): Extension<Actor>, Json(body): Json<Value>| {
                    let host = creation.host.lock().unwrap().clone();
                    async move { create(host, actor, body).await }
                },
            ),
        )
}

/// Create the Run `body` describes through `host`, as `actor`.
async fn create(
    host: Option<Arc<dyn Host>>,
    actor: Actor,
    body: Value,
) -> (StatusCode, Json<Value>) {
    let Some(host) = host else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "not started" })),
        );
    };
    let text = |key: &str| body[key].as_str().unwrap_or_default().to_string();
    let mut run = NewRun::new(text("pipeline"), text("input"));
    if !text("repo").is_empty() {
        let mut repo = RunRepo::new(text("repo"));
        if !text("source_branch").is_empty() {
            repo = repo.from_branch(text("source_branch"));
        }
        run = run.in_repo(repo);
    }
    if !text("name").is_empty() {
        run = run.named(text("name"));
    }
    if let Some(extensions) = body["extensions"].as_object() {
        for (id, value) in extensions {
            run = run.with_extension(id.clone(), value.clone());
        }
    }
    match host.create_run(&actor, run).await {
        Ok(run_id) => (StatusCode::CREATED, Json(json!({ "run_id": run_id }))),
        Err(refused) => (
            StatusCode::from_u16(refused.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(refused.body),
        ),
    }
}

/// The kind of what is created must be a Run for the data to reach it.
pub(super) fn is_run(kind: &ResourceKind) -> bool {
    *kind == ResourceKind::Run
}

#[test]
fn a_value_is_refused_by_its_first_refused_string() {
    let creation = Creation::default();
    assert_eq!(creation.validate(&json!({ "cle": "valeur" })), Ok(()));
    assert_eq!(
        creation.validate(&json!({ "cle": "refuse-me" })),
        Err("the test extension refuses cle=refuse-me".into())
    );
    assert_eq!(
        creation.validate(&json!("refuse")),
        Err("the test extension refuses refuse".into())
    );
    assert_eq!(creation.validate(&json!([1, "x"])), Ok(()));
}
