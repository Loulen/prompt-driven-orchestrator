//! The interop peers of the test extension (amendment #34 of ADR-0081): small
//! extensions that own an entity, the `item` (`<id>/item`), and mutate it
//! through the [`Host`] the core hands them at `start`, like `pm` does its
//! Tâches. Three of them installed together (`a`, `b`, `c`) interoperate
//! without naming each other:
//!
//! - **`authorize`** (the Host's): `POST /ext/<id>/items` asks every extension
//!   whether the actor may create the item; a refusal answers the core's
//!   `403` `operation_refused` with the refusing extension's reason, and
//!   nothing is stored. Same for `DELETE /ext/<id>/items/<item>`.
//! - **`publish`** and **`applied`**: a creation publishes `item_created`
//!   (on `/ws` and to the other extensions), then its `OperationApplied`.
//! - **`on_create`** (the Host's): the item stores the attributes the other
//!   extensions stamp on it, prefixed with their ids, and returns them.
//!
//! As an extension, each peer refuses every Operation on an extension's
//! resource once told to (`POST /ext/<id>/refusal {"refuses": true}`), stamps
//! `witness` (who created the entity) on whatever it sees created, and records
//! what `on_event` receives (`GET /ext/<id>/events`).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use pdo::extension_api::axum::extract::{Path, State};
use pdo::extension_api::axum::http::StatusCode;
use pdo::extension_api::axum::response::{IntoResponse, Response};
use pdo::extension_api::axum::routing::{get, post};
use pdo::extension_api::axum::{Extension as Ext, Json, Router};
use pdo::extension_api::{
    Actor, Attributes, Authorization, EntityRef, Extension, ExtensionContext, ExtensionEvent, Host,
    Manifest, NeutralHost, Operation, OperationApplied, ResourceKind, Verb,
};
use serde_json::{json, Value};

use super::verb_name;

/// The kind of entity every peer owns.
pub const ITEM: &str = "item";

/// The name of the event a creation publishes.
pub const ITEM_CREATED: &str = "item_created";

/// The ids of the three peers of the demo daemon.
pub const PEERS: [&str; 3] = ["a", "b", "c"];

/// The reason the peer `id` gives for refusing `operation`.
pub fn reason(id: &str, operation: &Operation) -> String {
    format!(
        "extension `{id}` refuses {} {}",
        verb_name(&operation.verb),
        operation.resource
    )
}

/// The test's side of one peer.
#[derive(Clone)]
pub struct PeerControl {
    id: &'static str,
    host: Arc<Mutex<Arc<dyn Host>>>,
    items: Arc<Mutex<BTreeMap<String, Value>>>,
    next: Arc<AtomicU64>,
    refuses: Arc<AtomicBool>,
    events: Arc<Mutex<Vec<Value>>>,
}

impl PeerControl {
    fn new(id: &'static str) -> Self {
        Self {
            id,
            host: Arc::new(Mutex::new(Arc::new(NeutralHost))),
            items: Arc::default(),
            next: Arc::default(),
            refuses: Arc::default(),
            events: Arc::default(),
        }
    }

    /// Refuse every Operation on an extension's resource, or stop refusing.
    pub fn set_refuses(&self, refuses: bool) {
        self.refuses.store(refuses, Ordering::Relaxed);
    }

    /// Every event `on_event` received, in order.
    pub fn events(&self) -> Vec<Value> {
        self.events.lock().unwrap().clone()
    }

    fn host(&self) -> Arc<dyn Host> {
        self.host.lock().unwrap().clone()
    }

    fn kind(&self) -> ResourceKind {
        ResourceKind::extension(self.id, ITEM)
    }

    fn items(&self) -> Vec<Value> {
        self.items.lock().unwrap().values().cloned().collect()
    }

    fn item(&self, id: &str) -> Option<Value> {
        self.items.lock().unwrap().get(id).cloned()
    }

    /// Create an item named `name` as `actor`, through the Host: `201` and
    /// the item, or the refusal.
    fn create(&self, actor: Actor, name: &str) -> Response {
        let host = self.host();
        let operation = Operation::new(self.kind(), Verb::Create);
        if let Err(denied) = host.authorize(&actor, &operation).into_result() {
            return denied.into_response();
        }
        let id = format!(
            "{}-{}",
            self.id,
            self.next.fetch_add(1, Ordering::Relaxed) + 1
        );
        let entity = EntityRef::new(self.kind(), id.clone());
        let attributes = host.on_create(&actor, &entity);
        let item = json!({ "id": id, "name": name, "attributes": attributes });
        self.items.lock().unwrap().insert(id, item.clone());
        host.publish(ExtensionEvent::new(ITEM_CREATED, item.clone()));
        host.applied(OperationApplied::new(actor, operation, Some(entity)));
        (StatusCode::CREATED, Json(item)).into_response()
    }

    /// Delete the item `id` as `actor`, through the Host: `204`, or the
    /// refusal.
    fn delete(&self, actor: Actor, id: &str) -> Response {
        if self.item(id).is_none() {
            return not_found(id);
        }
        let host = self.host();
        let operation = Operation::new(self.kind(), Verb::Delete).on(id);
        if let Err(denied) = host.authorize(&actor, &operation).into_result() {
            return denied.into_response();
        }
        self.items.lock().unwrap().remove(id);
        let entity = EntityRef::new(self.kind(), id);
        host.applied(OperationApplied::new(actor, operation, Some(entity)));
        StatusCode::NO_CONTENT.into_response()
    }
}

/// One interop peer.
pub struct Peer {
    control: PeerControl,
}

impl Peer {
    /// The peer `id`, and its control.
    pub fn new(id: &'static str) -> (Self, PeerControl) {
        let control = PeerControl::new(id);
        (
            Self {
                control: control.clone(),
            },
            control,
        )
    }

    /// The three peers `a`, `b`, `c`, boxed, and their controls.
    pub fn trio() -> (Vec<Box<dyn Extension>>, Vec<PeerControl>) {
        PEERS
            .iter()
            .map(|id| {
                let (peer, control) = Peer::new(id);
                (Box::new(peer) as Box<dyn Extension>, control)
            })
            .unzip()
    }
}

impl Extension for Peer {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            self.control.id,
            format!("Interop {}", self.control.id),
            "0.0.0",
        )
    }

    fn start(&self, context: &ExtensionContext) {
        *self.control.host.lock().unwrap() = context.host.clone();
    }

    fn authorize(&self, _actor: &Actor, operation: &Operation) -> Authorization {
        let refuses = self.control.refuses.load(Ordering::Relaxed);
        match &operation.resource {
            ResourceKind::Extension { .. } if refuses => {
                Authorization::deny(reason(self.control.id, operation))
            }
            _ => Authorization::Allow,
        }
    }

    fn on_create(&self, actor: &Actor, _entity: &EntityRef) -> Attributes {
        let witness = match actor {
            Actor::Identified { id, .. } => id.clone(),
            other => other.kind().to_string(),
        };
        Attributes::from([("witness".to_string(), witness)])
    }

    fn on_event(&self, event: &Value) {
        self.control.events.lock().unwrap().push(event.clone());
    }

    fn routes(&self) -> Option<Router> {
        Some(router(self.control.clone()))
    }
}

fn not_found(id: &str) -> Response {
    let body = json!({ "error": "item_not_found", "message": format!("no item `{id}`") });
    (StatusCode::NOT_FOUND, Json(body)).into_response()
}

fn router(control: PeerControl) -> Router {
    Router::new()
        .route(
            "/",
            get(|State(c): State<PeerControl>| async move {
                Json(json!({ "id": c.id, "refuses": c.refuses.load(Ordering::Relaxed) }))
            }),
        )
        .route(
            "/items",
            get(|State(c): State<PeerControl>| async move { Json(Value::Array(c.items())) }).post(
                |State(c): State<PeerControl>, Ext(actor): Ext<Actor>, Json(body): Json<Value>| async move {
                    let name = body["name"].as_str().unwrap_or("unnamed").to_string();
                    c.create(actor, &name)
                },
            ),
        )
        .route(
            "/items/{id}",
            get(
                |State(c): State<PeerControl>, Path(id): Path<String>| async move {
                    match c.item(&id) {
                        Some(item) => Json(item).into_response(),
                        None => not_found(&id),
                    }
                },
            )
            .delete(
                |State(c): State<PeerControl>, Ext(actor): Ext<Actor>, Path(id): Path<String>| async move {
                    c.delete(actor, &id)
                },
            ),
        )
        .route(
            "/refusal",
            post(
                |State(c): State<PeerControl>, Json(body): Json<Value>| async move {
                    let refuses = body["refuses"].as_bool().unwrap_or(true);
                    c.set_refuses(refuses);
                    Json(json!({ "id": c.id, "refuses": refuses }))
                },
            ),
        )
        .route(
            "/events",
            get(|State(c): State<PeerControl>| async move { Json(Value::Array(c.events())) }),
        )
        .with_state(control)
}
