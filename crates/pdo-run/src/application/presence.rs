//! The Presences on the Runs (API 0.12): each connection of the event stream
//! says which Run its tab shows; the core keeps who has which Run open
//! ([`PresenceBook`]) and announces each arrival and each departure on the
//! bus, hence on `/ws` (to the connections that may read the Run) and to the
//! extensions.
//!
//! A connection may only show a Run its Actor may read and the daemon knows:
//! anything else counts as showing no Run.

use pdo_extension_api::{Presence, PRESENCE_ARRIVED, PRESENCE_LEFT};
use pdo_kernel::clock::now_iso;
use pdo_kernel::Actor;
use serde_json::json;

use crate::domain::presence::{ConnectionId, PresenceBook, PresenceChange};
use crate::Runs;

/// The longest Run id a connection may name: longer is no Run.
const MAX_RUN_ID: usize = 200;

impl Runs {
    /// A new connection of the event stream, showing no Run yet.
    pub fn open_connection(&self) -> ConnectionId {
        self.next_connection
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// The connection `connection`, under `actor`, shows `run_id` now, or no
    /// Run: a Run it may not read, or that does not exist, is no Run.
    pub async fn show_run(&self, connection: ConnectionId, actor: &Actor, run_id: Option<&str>) {
        if self.presence_book().shows(connection, actor, run_id) {
            return;
        }
        let shown = match run_id {
            Some(run_id) if self.may_show(actor, run_id).await => Some(run_id),
            _ => None,
        };
        let mut book = self.presence_book();
        let changes = book.show(connection, actor, shown, &now_iso());
        self.announce(&book, changes);
    }

    /// The connection `connection` ended: its Actor leaves the Run it showed
    /// unless another of its connections still shows it.
    pub fn close_connection(&self, connection: ConnectionId) {
        let mut book = self.presence_book();
        let changes = book.close(connection);
        self.announce(&book, changes);
    }

    /// The Run `run_id` is forgotten: nobody is present on it any more.
    pub fn forget_presences(&self, run_id: &str) {
        self.presence_book().forget_run(run_id);
    }

    /// The Actors who have `run_id` open, by order of arrival.
    pub fn presences(&self, run_id: &str) -> Vec<Presence> {
        self.presence_book().presences(run_id)
    }

    /// Every Run someone has open that `actor` may read, with its Presences.
    pub async fn readable_presences(&self, actor: &Actor) -> Vec<(String, Vec<Presence>)> {
        let all = self.presence_book().all();
        let mut readable = Vec::with_capacity(all.len());
        for (run_id, presences) in all {
            if self.authorize_read(actor, &run_id).await.is_ok() {
                readable.push((run_id, presences));
            }
        }
        readable
    }

    async fn may_show(&self, actor: &Actor, run_id: &str) -> bool {
        if run_id.is_empty() || run_id.len() > MAX_RUN_ID {
            return false;
        }
        let known = match self.store.summary(run_id).await {
            Ok(Some(_)) => true,
            _ => self
                .store
                .events(run_id)
                .await
                .is_ok_and(|events| !events.is_empty()),
        };
        known && self.authorize_read(actor, run_id).await.is_ok()
    }

    fn presence_book(&self) -> std::sync::MutexGuard<'_, PresenceBook> {
        self.presences
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Each change on the bus, with the Run's Presences once it happened.
    /// Published under the lock of `book`: two changes of one Run reach the
    /// bus in the order of their snapshots.
    fn announce(&self, book: &PresenceBook, changes: Vec<PresenceChange>) {
        for change in changes {
            let presences = book.presences(change.run_id());
            let (kind, run_id, actor) = match change {
                PresenceChange::Arrived { run_id, actor } => (PRESENCE_ARRIVED, run_id, actor),
                PresenceChange::Left { run_id, actor } => (PRESENCE_LEFT, run_id, actor),
            };
            self.bus.publish(json!({
                "type": kind,
                "run_id": run_id,
                "actor": actor,
                "presences": presences,
            }));
        }
    }
}
