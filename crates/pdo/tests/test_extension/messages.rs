//! The messages of the test extension (API 0.20): the panel of its `topbar/end`
//! icon lists it, and the icon's counter counts it.
//!
//! - `GET /ext/test/messages`: the items, in order (`[{"id", "title"}]`);
//! - `DELETE /ext/test/messages/<id>`: deletes one (`404` for an unknown or
//!   already deleted one), `POST /ext/test/messages/<id>/restore` brings it back
//!   at its place (what the toast's « Undo » calls);
//! - `GET /ext/test/messages/counter`: the [`PageCounter`] of the icon, the
//!   count of the items;
//! - `PUT /ext/test/messages {"count": n, "title"?: …}`: replaces the items by
//!   `n` items (`Item 1`…, or `title` followed by the number), to try many
//!   items or a long title. Three to start with.
//!
//! Each change publishes [`MESSAGES_CHANGED`], so the core reads the counter
//! again.

use std::sync::{Arc, Mutex};

use pdo::extension_api::axum::extract::Path;
use pdo::extension_api::axum::http::StatusCode;
use pdo::extension_api::axum::routing::{delete, get, post};
use pdo::extension_api::axum::{Json, Router};
use pdo::extension_api::{ExtensionEvent, PageCounter};
use serde_json::{json, Value};

use super::creation::Creation;

/// The event a change of the messages publishes.
pub const MESSAGES_CHANGED: &str = "messages_changed";

/// The messages it starts with.
const SEED: [&str; 3] = [
    "Review the release notes",
    "Answer the question",
    "Plan the demo",
];

#[derive(Clone)]
struct Item {
    id: String,
    title: String,
    deleted: bool,
}

#[derive(Clone)]
struct Messages {
    items: Arc<Mutex<Vec<Item>>>,
    creation: Creation,
}

impl Messages {
    fn new(creation: Creation) -> Self {
        let messages = Self {
            items: Arc::default(),
            creation,
        };
        messages.fill(SEED.iter().map(|t| t.to_string()).collect());
        messages
    }

    fn fill(&self, titles: Vec<String>) {
        *self.items.lock().unwrap() = titles
            .into_iter()
            .enumerate()
            .map(|(i, title)| Item {
                id: format!("item-{}", i + 1),
                title,
                deleted: false,
            })
            .collect();
    }

    fn live(&self) -> Vec<Value> {
        self.items
            .lock()
            .unwrap()
            .iter()
            .filter(|item| !item.deleted)
            .map(|item| json!({ "id": item.id, "title": item.title }))
            .collect()
    }

    /// Mark `id` deleted (or not): whether it changed.
    fn mark(&self, id: &str, deleted: bool) -> bool {
        let mut items = self.items.lock().unwrap();
        match items.iter_mut().find(|item| item.id == id) {
            Some(item) if item.deleted != deleted => {
                item.deleted = deleted;
                true
            }
            _ => false,
        }
    }

    fn changed(&self) {
        if let Some(host) = self.creation.host() {
            host.publish(ExtensionEvent::new(
                MESSAGES_CHANGED,
                json!({ "count": self.live().len() }),
            ));
        }
    }

    /// The counter of the panel's icon.
    fn counter(&self) -> PageCounter {
        let count = self.live().len();
        let detail = match count {
            1 => "1 item".to_string(),
            n => format!("{n} items"),
        };
        PageCounter::new(count as u32).with_detail(detail)
    }
}

pub(super) fn routes(creation: Creation) -> Router {
    let messages = Messages::new(creation);
    let (list, fill, remove, restore, counter) = (
        messages.clone(),
        messages.clone(),
        messages.clone(),
        messages.clone(),
        messages,
    );
    Router::new()
        .route(
            "/messages",
            get(move || {
                let items = list.live();
                async move { Json(Value::Array(items)) }
            })
            .put(move |Json(body): Json<Value>| {
                let count = body["count"].as_u64().unwrap_or(0) as usize;
                let titles = (1..=count)
                    .map(|n| match body["title"].as_str() {
                        Some(title) => format!("{title} {n}"),
                        None => format!("Item {n}"),
                    })
                    .collect();
                fill.fill(titles);
                fill.changed();
                let items = fill.live();
                async move { Json(Value::Array(items)) }
            }),
        )
        .route(
            "/messages/counter",
            get(move || {
                let counter = counter.counter();
                async move { Json(json!(counter)) }
            }),
        )
        .route(
            "/messages/{id}",
            delete(move |Path(id): Path<String>| {
                let answer = toggled(&remove, &id, true);
                async move { answer }
            }),
        )
        .route(
            "/messages/{id}/restore",
            post(move |Path(id): Path<String>| {
                let answer = toggled(&restore, &id, false);
                async move { answer }
            }),
        )
}

/// Delete or restore `id`: `200` and the items, `404` when nothing changed.
fn toggled(messages: &Messages, id: &str, deleted: bool) -> (StatusCode, Json<Value>) {
    if !messages.mark(id, deleted) {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("no item {id} to change") })),
        );
    }
    messages.changed();
    (StatusCode::OK, Json(Value::Array(messages.live())))
}
