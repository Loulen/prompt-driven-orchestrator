//! The Presences on the Runs (API 0.12): each connection of the event stream
//! says which Run its tab shows; the core keeps who has which Run open
//! ([`PresenceBook`]) and announces each arrival and each departure on the
//! bus, hence on `/ws` (to the connections that may read the Run) and to the
//! extensions.
//!
//! A connection may only show a Run its Actor may read and the daemon knows:
//! anything else counts as showing no Run.
//!
//! A connection that names its tab replaces the one that tab still had open
//! (API 0.23): the old one's Presence falls at once, and its relay is told to
//! end ([`StreamConnection::replaced`]).

use std::sync::Arc;

use pdo_extension_api::{Presence, PRESENCE_ARRIVED, PRESENCE_LEFT};
use pdo_kernel::clock::now_iso;
use pdo_kernel::Actor;
use serde_json::json;

use tokio::sync::Notify;

use crate::domain::presence::{ConnectionId, PresenceBook, PresenceChange};
use crate::domain::stream_liveness::StreamLiveness;
use crate::Runs;

/// The longest Run id a connection may name: longer is no Run.
const MAX_RUN_ID: usize = 200;

/// The longest tab identity a connection may name: longer, or with a
/// character outside `[A-Za-z0-9_-]`, is no tab.
const MAX_TAB: usize = 64;

/// A connection of the event stream, just opened.
pub struct StreamConnection {
    pub id: ConnectionId,
    /// Notified when a newer connection of the same tab replaced this one:
    /// its relay ends then.
    pub replaced: Arc<Notify>,
}

impl Runs {
    /// A new connection of the event stream of `actor`, showing no Run yet,
    /// from the tab `tab` if it names one: it replaces the connection that
    /// tab still had open, whose Presence falls now.
    pub fn open_connection(&self, actor: &Actor, tab: Option<&str>) -> StreamConnection {
        let id = self
            .next_connection
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let replaced = Arc::new(Notify::new());
        self.stream_ends().insert(id, replaced.clone());
        let tab = tab.filter(|tab| is_tab(tab));
        let mut book = self.presence_book();
        let opening = book.open(id, actor, tab);
        self.announce(&book, opening.changes);
        drop(book);
        if let Some(old) = opening.replaced {
            if let Some(end) = self.stream_ends().remove(&old) {
                end.notify_one();
            }
        }
        StreamConnection { id, replaced }
    }

    /// How the event stream pings its connections, and when one is mute.
    pub fn stream_liveness(&self) -> StreamLiveness {
        *self
            .stream_liveness
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Test seam: the stream's liveness for the connections opened from now.
    pub fn set_stream_liveness(&self, liveness: StreamLiveness) {
        *self
            .stream_liveness
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = liveness;
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
        self.stream_ends().remove(&connection);
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

    fn stream_ends(
        &self,
    ) -> std::sync::MutexGuard<'_, std::collections::HashMap<ConnectionId, Arc<Notify>>> {
        self.stream_ends
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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

/// Whether `tab` is a tab identity a connection may name.
fn is_tab(tab: &str) -> bool {
    !tab.is_empty()
        && tab.len() <= MAX_TAB
        && tab
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tab_identity_is_short_and_plain() {
        assert!(is_tab("0b5e3c1a-7f2d-4c55-9a8e-1f2e3d4c5b6a"));
        assert!(is_tab("tab_1"));
        assert!(!is_tab(""));
        assert!(!is_tab(&"a".repeat(MAX_TAB + 1)));
        assert!(!is_tab("a b"));
        assert!(!is_tab("é"));
    }
}
