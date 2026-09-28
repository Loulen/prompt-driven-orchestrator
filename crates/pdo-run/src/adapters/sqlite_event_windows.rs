//! [`EventWindows`] over the event log of `pdo.db`: the cross-Run reads the
//! Stats cockpit aggregates, as indexed SQL (`idx_events_kind_ts`, and the
//! cohort join index) rather than a replay of every Run.

use std::collections::BTreeSet;

use anyhow::Result;

use super::sqlite_event_store::SqliteEventStore;
use crate::application::ports::{BoxFuture, EventWindow, EventWindows, NodeStart, RunStart};

/// The SQL fragment that narrows a Run-keyed query to the « completed runs
/// only » cohort (#810), or nothing at all. Written once so every read of a
/// window describes the same Runs. `run_alias` names the table the caller's
/// rows are keyed by.
fn completed_only_sql(completed_only: bool, run_alias: &str) -> String {
    if completed_only {
        format!(
            " AND EXISTS (SELECT 1 FROM events done \
               WHERE done.run_id = {run_alias}.run_id AND done.kind = 'run_completed')"
        )
    } else {
        String::new()
    }
}

/// A stored payload, `Null` when absent or unreadable.
fn payload_of(text: Option<&str>) -> serde_json::Value {
    text.and_then(|payload| serde_json::from_str(payload).ok())
        .unwrap_or(serde_json::Value::Null)
}

async fn count_by_period(
    db: &sqlx::SqlitePool,
    kind: &str,
    fmt: &str,
    window: EventWindow<'_>,
) -> Result<Vec<(String, i64)>> {
    Ok(sqlx::query_as::<_, (String, i64)>(&format!(
        "SELECT strftime(?, e.ts) AS bucket, COUNT(*) AS count \
         FROM events e WHERE e.kind = ? AND e.ts >= ? AND e.ts < ?{} \
         GROUP BY bucket ORDER BY bucket",
        completed_only_sql(window.completed_only, "e")
    ))
    .bind(fmt)
    .bind(kind)
    .bind(window.from)
    .bind(window.to)
    .fetch_all(db)
    .await?)
}

async fn run_starts(
    db: &sqlx::SqlitePool,
    fmt: Option<&str>,
    window: EventWindow<'_>,
) -> Result<Vec<RunStart>> {
    let completed = completed_only_sql(window.completed_only, "e");
    let rows = match fmt {
        Some(fmt) => {
            sqlx::query_as::<_, (String, String, Option<String>, Option<String>)>(&format!(
                "SELECT e.run_id, e.ts, strftime(?, e.ts) AS bucket, e.payload \
                 FROM events e WHERE e.kind = 'run_started' AND e.ts >= ? AND e.ts < ?{completed} \
                 ORDER BY e.ts"
            ))
            .bind(fmt)
            .bind(window.from)
            .bind(window.to)
            .fetch_all(db)
            .await?
        }
        None => {
            sqlx::query_as::<_, (String, String, Option<String>, Option<String>)>(&format!(
                "SELECT e.run_id, e.ts, NULL AS bucket, e.payload \
                 FROM events e WHERE e.kind = 'run_started' AND e.ts >= ? AND e.ts < ?{completed} \
                 ORDER BY e.ts"
            ))
            .bind(window.from)
            .bind(window.to)
            .fetch_all(db)
            .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(|(run_id, ts, bucket, payload)| RunStart {
            run_id,
            ts,
            bucket,
            payload: payload_of(payload.as_deref()),
        })
        .collect())
}

async fn node_starts(
    db: &sqlx::SqlitePool,
    fmt: &str,
    window: EventWindow<'_>,
) -> Result<Vec<NodeStart>> {
    type Row = (
        String,
        String,
        String,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<i64>,
        Option<String>,
    );
    let rows = sqlx::query_as::<_, Row>(&format!(
        "SELECT strftime(?, cohort.ts), cohort.ts, cohort.run_id, cohort.payload, event.id, \
                event.node_id, event.iter, event.payload \
         FROM events cohort \
         JOIN events event ON event.run_id = cohort.run_id AND event.kind = 'node_started' \
         WHERE cohort.kind = 'run_started' AND cohort.ts >= ? AND cohort.ts < ?{} \
         ORDER BY cohort.ts, event.id",
        completed_only_sql(window.completed_only, "cohort")
    ))
    .bind(fmt)
    .bind(window.from)
    .bind(window.to)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(bucket, run_ts, run_id, run_payload, event_id, node_id, iter, payload)| NodeStart {
                bucket,
                run_id,
                run_ts,
                run_payload: payload_of(run_payload.as_deref()),
                event_id,
                node_id,
                iter,
                payload: payload_of(payload.as_deref()),
            },
        )
        .collect())
}

async fn nodes_started_under(
    db: &sqlx::SqlitePool,
    pipeline: &str,
    unknown: &str,
) -> Result<BTreeSet<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT ns.node_id FROM events ns \
         JOIN events rs ON rs.run_id = ns.run_id AND rs.kind = 'run_started' \
         WHERE ns.kind = 'node_started' AND ns.node_id IS NOT NULL \
           AND COALESCE(json_extract(rs.payload, '$.pipeline_id'), \
                        json_extract(rs.payload, '$.pipeline_name'), ?) = ?",
    )
    .bind(unknown)
    .bind(pipeline)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

async fn latest_run_start(
    db: &sqlx::SqlitePool,
    pipeline: &str,
    unknown: &str,
) -> Result<Option<serde_json::Value>> {
    let payload: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT payload FROM events WHERE kind = 'run_started' \
           AND COALESCE(json_extract(payload, '$.pipeline_id'), \
                        json_extract(payload, '$.pipeline_name'), ?) = ? \
         ORDER BY ts DESC LIMIT 1",
    )
    .bind(unknown)
    .bind(pipeline)
    .fetch_optional(db)
    .await?;
    Ok(payload
        .map(|(text,)| payload_of(text.as_deref()))
        .filter(|payload| !payload.is_null()))
}

impl EventWindows for SqliteEventStore {
    fn count_by_period<'a>(
        &'a self,
        kind: &'a str,
        bucket_format: &'a str,
        window: EventWindow<'a>,
    ) -> BoxFuture<'a, Result<Vec<(String, i64)>>> {
        Box::pin(count_by_period(self.pool(), kind, bucket_format, window))
    }

    fn run_starts<'a>(
        &'a self,
        bucket_format: Option<&'a str>,
        window: EventWindow<'a>,
    ) -> BoxFuture<'a, Result<Vec<RunStart>>> {
        Box::pin(run_starts(self.pool(), bucket_format, window))
    }

    fn node_starts<'a>(
        &'a self,
        bucket_format: &'a str,
        window: EventWindow<'a>,
    ) -> BoxFuture<'a, Result<Vec<NodeStart>>> {
        Box::pin(node_starts(self.pool(), bucket_format, window))
    }

    fn nodes_started_under<'a>(
        &'a self,
        pipeline: &'a str,
        unknown: &'a str,
    ) -> BoxFuture<'a, Result<BTreeSet<String>>> {
        Box::pin(nodes_started_under(self.pool(), pipeline, unknown))
    }

    fn latest_run_start<'a>(
        &'a self,
        pipeline: &'a str,
        unknown: &'a str,
    ) -> BoxFuture<'a, Result<Option<serde_json::Value>>> {
        Box::pin(latest_run_start(self.pool(), pipeline, unknown))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::sqlite_event_store::init;

    async fn store() -> SqliteEventStore {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        init(&db).await.unwrap();
        SqliteEventStore::new(db)
    }

    async fn insert(
        store: &SqliteEventStore,
        run: &str,
        ts: &str,
        kind: &str,
        node: Option<&str>,
        payload: serde_json::Value,
    ) {
        sqlx::query(
            "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) \
             VALUES (?, ?, ?, ?, 1, ?)",
        )
        .bind(run)
        .bind(ts)
        .bind(kind)
        .bind(node)
        .bind(payload.to_string())
        .execute(store.pool())
        .await
        .unwrap();
    }

    fn window(completed_only: bool) -> EventWindow<'static> {
        EventWindow {
            from: "2026-07-15T00:00:00Z",
            to: "2026-07-17T00:00:00Z",
            completed_only,
        }
    }

    async fn seeded() -> SqliteEventStore {
        let store = store().await;
        let p1 = serde_json::json!({ "pipeline_id": "p1", "pipeline_name": "One" });
        let named = serde_json::json!({ "pipeline_name": "Named" });
        insert(
            &store,
            "r1",
            "2026-07-15T09:00:00Z",
            "run_started",
            None,
            p1,
        )
        .await;
        insert(
            &store,
            "r1",
            "2026-07-15T09:01:00Z",
            "node_started",
            Some("a"),
            serde_json::json!({}),
        )
        .await;
        insert(
            &store,
            "r1",
            "2026-07-15T10:00:00Z",
            "run_completed",
            None,
            serde_json::json!({}),
        )
        .await;
        insert(
            &store,
            "r2",
            "2026-07-16T09:00:00Z",
            "run_started",
            None,
            named,
        )
        .await;
        insert(
            &store,
            "r2",
            "2026-07-16T09:01:00Z",
            "node_started",
            Some("b"),
            serde_json::json!({}),
        )
        .await;
        insert(
            &store,
            "r2",
            "2026-07-16T09:02:00Z",
            "run_failed",
            None,
            serde_json::json!({}),
        )
        .await;
        insert(
            &store,
            "r3",
            "2026-07-18T09:00:00Z",
            "run_started",
            None,
            serde_json::json!({}),
        )
        .await;
        insert(
            &store,
            "r3",
            "2026-07-18T09:01:00Z",
            "node_started",
            Some("c"),
            serde_json::json!({}),
        )
        .await;
        store
    }

    #[tokio::test]
    async fn a_window_counts_per_period_and_narrows_to_the_completed_runs() {
        let store = seeded().await;
        let all = store
            .count_by_period("run_started", "%Y-%m-%d", window(false))
            .await
            .unwrap();
        assert_eq!(
            all,
            vec![("2026-07-15".to_string(), 1), ("2026-07-16".to_string(), 1)]
        );
        let completed = store
            .count_by_period("run_started", "%Y-%m-%d", window(true))
            .await
            .unwrap();
        assert_eq!(completed, vec![("2026-07-15".to_string(), 1)]);
    }

    #[tokio::test]
    async fn the_cohort_and_its_node_starts_are_the_runs_started_in_the_window() {
        let store = seeded().await;
        let starts = store
            .run_starts(Some("%Y-%m"), window(false))
            .await
            .unwrap();
        assert_eq!(
            starts.iter().map(|s| s.run_id.as_str()).collect::<Vec<_>>(),
            ["r1", "r2"]
        );
        assert_eq!(starts[0].bucket.as_deref(), Some("2026-07"));
        assert_eq!(starts[0].payload["pipeline_id"], "p1");
        let bare = store.run_starts(None, window(true)).await.unwrap();
        assert_eq!(bare.len(), 1);
        assert_eq!(bare[0].bucket, None);

        let nodes = store.node_starts("%Y-%m-%d", window(false)).await.unwrap();
        assert_eq!(
            nodes
                .iter()
                .map(|n| (n.run_id.as_str(), n.node_id.as_deref()))
                .collect::<Vec<_>>(),
            [("r1", Some("a")), ("r2", Some("b"))]
        );
        assert_eq!(nodes[1].run_payload["pipeline_name"], "Named");
    }

    #[tokio::test]
    async fn a_pipelines_history_reads_by_id_then_name_then_the_unknown_key() {
        let store = seeded().await;
        let under = |pipeline: &'static str| {
            let store = &store;
            async move {
                store
                    .nodes_started_under(pipeline, "(unknown)")
                    .await
                    .unwrap()
            }
        };
        assert_eq!(under("p1").await, BTreeSet::from(["a".to_string()]));
        assert_eq!(under("Named").await, BTreeSet::from(["b".to_string()]));
        assert_eq!(under("(unknown)").await, BTreeSet::from(["c".to_string()]));
        let latest = store.latest_run_start("p1", "(unknown)").await.unwrap();
        assert_eq!(latest.unwrap()["pipeline_name"], "One");
        assert_eq!(
            store.latest_run_start("nope", "(unknown)").await.unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn init_installs_the_idempotent_session_cohort_join_index() {
        let store = store().await;
        init(store.pool()).await.unwrap();
        let columns = sqlx::query_scalar::<_, String>(
            "SELECT name FROM pragma_index_info('idx_events_run_kind_id') ORDER BY seqno",
        )
        .fetch_all(store.pool())
        .await
        .unwrap();
        assert_eq!(columns, vec!["run_id", "kind", "id"]);
    }
}
