//! One shared projection of every Run, rebuilt only when the event store changes
//! (#1009).
//!
//! Many daemon paths need "every Run, projected":
//!
//! - `GET /runs`, fetched again by every open tab on every WebSocket event;
//! - `GET /runs/{id}` (child-awaiting overlay) and `GET /runs/{id}/children`,
//!   polled every 3 s;
//! - the `pdo run wait` long-poll, every 500 ms for as long as an orchestrator
//!   node waits on its children (hours, sometimes);
//! - the stale sweep (30 s) and `retry_waiting_nodes` (every slot release).
//!
//! Each of them used to reload and re-project the whole store: a few thousand
//! events, parsed and allocated again on every call. Measured in production, two
//! orchestrators in `pdo run wait` kept the daemon at ~85 % of a core, and when the
//! machine slowed down the calls piled up faster than they finished until the OOM
//! killer took the daemon (7.4 GB peak).
//!
//! The store is append-only apart from `DELETE … WHERE run_id` (forget), ids are
//! AUTOINCREMENT (never reused) and [`event_log::project`] is pure — it never reads
//! the clock. A projection therefore stays exact for as long as the store's
//! signature `(COUNT(*), MAX(id))` is unchanged: an append moves `MAX(id)`, a delete
//! moves `COUNT(*)`.
//!
//! A miss rebuilds under an async mutex, so concurrent callers wait for the one
//! rebuild instead of each running their own. That single-flight is the point: the
//! cost of a burst no longer grows with the number of callers.

use std::sync::Arc;

use anyhow::{Context, Result};

use crate::child_awaiting;
use crate::event_log::{self, RunState};

/// `(COUNT(*), MAX(id))` of the `events` table — see the module docs for why
/// this is enough to tell that a projection went stale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StoreSignature {
    count: i64,
    max_id: i64,
}

struct Snapshot {
    signature: StoreSignature,
    runs: Arc<Vec<RunState>>,
    overlaid: Arc<Vec<RunState>>,
}

/// Every Run of the store, projected, newest Run id first (the order
/// `load_all_run_ids` has always returned). Shared, read-only: a caller that
/// needs to mutate a state clones that one state.
#[derive(Clone)]
pub(crate) struct ProjectedRuns {
    /// Plain projections, exactly what `event_log::project` returns.
    pub(crate) runs: Arc<Vec<RunState>>,
    /// The same projections with the child-awaiting overlay applied
    /// ([`child_awaiting::overlay`]) — what every HTTP read serves.
    pub(crate) overlaid: Arc<Vec<RunState>>,
}

#[derive(Default)]
pub(crate) struct RunProjectionCache {
    slot: tokio::sync::Mutex<Option<Snapshot>>,
}

impl RunProjectionCache {
    /// The projection of every Run, served from the cache while the store is
    /// unchanged, rebuilt once otherwise.
    pub(crate) async fn get(&self, db: &sqlx::SqlitePool) -> Result<ProjectedRuns> {
        // The signature is read under the lock: a caller that queued behind a
        // rebuild then sees the rebuilt snapshot as a hit instead of rebuilding
        // again.
        let mut slot = self.slot.lock().await;
        let signature = store_signature(db).await?;
        if let Some(snapshot) = slot.as_ref() {
            if snapshot.signature == signature {
                return Ok(ProjectedRuns {
                    runs: snapshot.runs.clone(),
                    overlaid: snapshot.overlaid.clone(),
                });
            }
        }
        // Read after the signature: the projection is at least as fresh as the
        // signature it is filed under. An event appended in between only makes the
        // next call rebuild once more — never serve something older.
        let runs = project_every_run(db).await?;
        let mut overlaid = runs.clone();
        child_awaiting::overlay(&mut overlaid);
        let snapshot = Snapshot {
            signature,
            runs: Arc::new(runs),
            overlaid: Arc::new(overlaid),
        };
        let projected = ProjectedRuns {
            runs: snapshot.runs.clone(),
            overlaid: snapshot.overlaid.clone(),
        };
        *slot = Some(snapshot);
        Ok(projected)
    }
}

async fn store_signature(db: &sqlx::SqlitePool) -> Result<StoreSignature> {
    let (count, max_id): (i64, Option<i64>) =
        sqlx::query_as("SELECT COUNT(*), MAX(id) FROM events")
            .fetch_one(db)
            .await
            .context("failed to read the event store signature")?;
    Ok(StoreSignature {
        count,
        max_id: max_id.unwrap_or(0),
    })
}

/// Project every Run in one read of the store, in the order `load_all_run_ids`
/// returns them (Run id descending), each Run's events in id order — the same
/// slices `load_events` hands to `event_log::project`. A Run whose log projects to
/// nothing is skipped, as every per-Run loop did.
async fn project_every_run(db: &sqlx::SqlitePool) -> Result<Vec<RunState>> {
    let rows = sqlx::query_as::<_, crate::EventRow>(
        "SELECT id, run_id, ts, kind, node_id, iter, payload FROM events \
         ORDER BY run_id DESC, id",
    )
    .fetch_all(db)
    .await
    .context("failed to load events")?;

    let mut runs = Vec::new();
    let mut current: Vec<event_log::Event> = Vec::new();
    for row in rows {
        let event = row.into_event();
        if current
            .first()
            .is_some_and(|first| first.run_id != event.run_id)
        {
            runs.extend(event_log::project(&current));
            current.clear();
        }
        current.push(event);
    }
    runs.extend(event_log::project(&current));
    Ok(runs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_log::{Event, EventKind};

    async fn store() -> (sqlx::SqlitePool, tokio::sync::broadcast::Sender<Event>) {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::init_db(&db).await.unwrap();
        let (tx, _) = tokio::sync::broadcast::channel(64);
        (db, tx)
    }

    fn event(
        run_id: &str,
        kind: EventKind,
        node: Option<&str>,
        payload: Option<serde_json::Value>,
    ) -> Event {
        Event {
            id: None,
            run_id: run_id.into(),
            ts: event_log::now_iso(),
            kind,
            node_id: node.map(str::to_string),
            iter: node.map(|_| 1),
            payload,
        }
    }

    async fn append(db: &sqlx::SqlitePool, tx: &tokio::sync::broadcast::Sender<Event>, e: Event) {
        crate::append_event_with(db, tx, &e).await.unwrap();
    }

    /// A Run started under `parent` (when given), with one node started.
    async fn start_run(
        db: &sqlx::SqlitePool,
        tx: &tokio::sync::broadcast::Sender<Event>,
        run_id: &str,
        parent: Option<&str>,
    ) {
        let payload = match parent {
            Some(p) => serde_json::json!({
                "pipeline_name": "pipe",
                "parent_run_id": p,
                "parent_node_id": "spawner",
            }),
            None => serde_json::json!({ "pipeline_name": "pipe" }),
        };
        append(
            db,
            tx,
            event(run_id, EventKind::RunStarted, None, Some(payload)),
        )
        .await;
        append(
            db,
            tx,
            event(run_id, EventKind::NodeStarted, Some("spawner"), None),
        )
        .await;
    }

    /// What every per-Run loop computed before the cache: `load_all_run_ids`,
    /// then `load_events` + `project` per Run, then the overlay.
    async fn per_run_projection(db: &sqlx::SqlitePool) -> (Vec<RunState>, Vec<RunState>) {
        let mut runs = Vec::new();
        for id in crate::load_all_run_ids(db).await.unwrap() {
            let events = crate::load_events(db, &id).await.unwrap();
            runs.extend(event_log::project(&events));
        }
        let mut overlaid = runs.clone();
        child_awaiting::overlay(&mut overlaid);
        (runs, overlaid)
    }

    fn json(runs: &[RunState]) -> serde_json::Value {
        serde_json::to_value(runs).unwrap()
    }

    #[tokio::test]
    async fn matches_the_per_run_projection_overlay_included() {
        let (db, tx) = store().await;
        start_run(&db, &tx, "20261105-100000-aaaaaaa", None).await;
        start_run(
            &db,
            &tx,
            "20261105-100100-bbbbbbb",
            Some("20261105-100000-aaaaaaa"),
        )
        .await;
        start_run(
            &db,
            &tx,
            "20261105-100200-ccccccc",
            Some("20261105-100100-bbbbbbb"),
        )
        .await;
        // The grandchild waits on its user: the overlay lifts child and parent.
        append(
            &db,
            &tx,
            event(
                "20261105-100200-ccccccc",
                EventKind::NodeAwaitingUser,
                Some("spawner"),
                Some(serde_json::json!({ "cause": "declared", "message": "?" })),
            ),
        )
        .await;
        start_run(&db, &tx, "20261105-090000-ddddddd", None).await;

        let cache = RunProjectionCache::default();
        let projected = cache.get(&db).await.unwrap();
        let (runs, overlaid) = per_run_projection(&db).await;

        assert_eq!(projected.runs.len(), 4);
        assert_eq!(json(&projected.runs), json(&runs));
        assert_eq!(json(&projected.overlaid), json(&overlaid));
        // The overlay did something, so the comparison above is not vacuous.
        assert_ne!(json(&overlaid), json(&runs));
    }

    #[tokio::test]
    async fn an_unchanged_store_is_served_without_a_rebuild() {
        let (db, tx) = store().await;
        start_run(&db, &tx, "20261105-100000-aaaaaaa", None).await;

        let cache = RunProjectionCache::default();
        let first = cache.get(&db).await.unwrap();
        let second = cache.get(&db).await.unwrap();

        assert!(Arc::ptr_eq(&first.runs, &second.runs));
        assert!(Arc::ptr_eq(&first.overlaid, &second.overlaid));
    }

    #[tokio::test]
    async fn an_appended_event_is_seen_by_the_next_read() {
        let (db, tx) = store().await;
        start_run(&db, &tx, "20261105-100000-aaaaaaa", None).await;
        let cache = RunProjectionCache::default();
        let before = cache.get(&db).await.unwrap();

        append(
            &db,
            &tx,
            event(
                "20261105-100000-aaaaaaa",
                EventKind::NodeCompleted,
                Some("spawner"),
                None,
            ),
        )
        .await;
        start_run(&db, &tx, "20261105-110000-eeeeeee", None).await;
        let after = cache.get(&db).await.unwrap();

        assert!(!Arc::ptr_eq(&before.runs, &after.runs));
        assert_eq!(json(&after.runs), json(&per_run_projection(&db).await.0));
        assert_eq!(after.runs[0].run_id, "20261105-110000-eeeeeee");
    }

    #[tokio::test]
    async fn a_forgotten_run_drops_out_of_the_next_read() {
        let (db, tx) = store().await;
        start_run(&db, &tx, "20261105-100000-aaaaaaa", None).await;
        start_run(&db, &tx, "20261105-110000-eeeeeee", None).await;
        let cache = RunProjectionCache::default();
        assert_eq!(cache.get(&db).await.unwrap().runs.len(), 2);

        // Forget deletes the newest Run's events: COUNT(*) moves, MAX(id) may not.
        sqlx::query("DELETE FROM events WHERE run_id = ?")
            .bind("20261105-110000-eeeeeee")
            .execute(&db)
            .await
            .unwrap();
        let after = cache.get(&db).await.unwrap();

        assert_eq!(after.runs.len(), 1);
        assert_eq!(after.runs[0].run_id, "20261105-100000-aaaaaaa");
    }

    #[tokio::test]
    async fn concurrent_reads_of_a_changed_store_rebuild_once() {
        let (db, tx) = store().await;
        start_run(&db, &tx, "20261105-100000-aaaaaaa", None).await;
        let cache = Arc::new(RunProjectionCache::default());

        let reads: Vec<_> = (0..8)
            .map(|_| {
                let cache = cache.clone();
                let db = db.clone();
                tokio::spawn(async move { cache.get(&db).await.unwrap() })
            })
            .collect();
        let mut results = Vec::new();
        for read in reads {
            results.push(read.await.unwrap());
        }

        // Every caller got the one snapshot the first of them built.
        assert!(results
            .iter()
            .all(|r| Arc::ptr_eq(&r.runs, &results[0].runs)));
    }
}
