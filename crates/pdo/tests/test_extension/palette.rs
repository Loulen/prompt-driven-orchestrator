//! The counter of the test extension's `Inbox`, a global Action of the
//! command field (API 0.20): the core's fixture for the counter of a
//! `palette` Action.
//!
//! - `GET /ext/test/inbox`: the [`PageCounter`] the Action's row shows, in the
//!   accent colour; [`START`] until a test sets it;
//! - `PUT /ext/test/inbox` (`{"count": n}`): sets it, then publishes
//!   [`INBOX_CHANGED`], so an open command field reads it again.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use pdo::extension_api::axum::routing::get;
use pdo::extension_api::axum::{Json, Router};
use pdo::extension_api::{ExtensionEvent, PageCounter};
use serde_json::{json, Value};

use super::creation::Creation;

/// The id of the command of the `Inbox` Action.
pub const INBOX: &str = "test.inbox";

/// The name of the event a change of the count publishes.
pub const INBOX_CHANGED: &str = "inbox_changed";

/// The count the Action shows before a test sets it.
pub const START: u32 = 3;

/// The count of the `Inbox`, shared by its two routes.
#[derive(Clone)]
pub struct Inbox(Arc<AtomicU32>);

impl Default for Inbox {
    fn default() -> Self {
        Self(Arc::new(AtomicU32::new(START)))
    }
}

impl Inbox {
    pub fn count(&self) -> u32 {
        self.0.load(Ordering::Relaxed)
    }

    pub fn set(&self, count: u32) {
        self.0.store(count, Ordering::Relaxed);
    }

    /// What its route answers: the count, accented, `n unread` as detail.
    pub fn counter(&self) -> PageCounter {
        let count = self.count();
        PageCounter::new(count)
            .accented(true)
            .with_detail(format!("{count} unread"))
    }
}

pub(super) fn routes(inbox: Inbox, creation: Creation) -> Router {
    let (read, write) = (inbox.clone(), inbox);
    Router::new().route(
        "/inbox",
        get(move || {
            let counter = read.counter();
            async move { Json(json!(counter)) }
        })
        .put(move |Json(body): Json<Value>| {
            let count = body["count"].as_u64().unwrap_or(0) as u32;
            write.set(count);
            if let Some(host) = creation.host() {
                host.publish(ExtensionEvent::new(
                    INBOX_CHANGED,
                    json!({ "count": count }),
                ));
            }
            let counter = write.counter();
            async move { Json(json!(counter)) }
        }),
    )
}
