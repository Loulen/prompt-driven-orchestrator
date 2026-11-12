//! The port of the stats context that the Triggers answer: the fires of a
//! window (#377), read in the trigger context's tables (`pdo-trigger`, #927).
//! Neither context depends on the other, so the composition root reads one for
//! the other.

use std::collections::BTreeSet;

use pdo_stats::application::ports::{BoxFuture, TriggerFires, TriggerProject};
use pdo_stats::domain::overview::fires::DELETED_TRIGGER;
use pdo_stats::domain::overview::TriggersCreatedRuns;

/// The Triggers of the daemon's database.
pub(crate) struct TriggerTableFires {
    pub(crate) db: sqlx::SqlitePool,
}

/// The Triggers of `triggers` as one JSON array, the parameter of
/// [`ONLY_THESE`]; `None` for every Trigger.
fn triggers_of(triggers: Option<&BTreeSet<String>>) -> Option<String> {
    triggers.map(|ids| serde_json::Value::from(ids.iter().cloned().collect::<Vec<_>>()).to_string())
}

/// The SQL fragment narrowing to the Triggers of a JSON array parameter.
const ONLY_THESE: &str = "IN (SELECT value FROM json_each(?))";

/// Fires per pipeline in the window. `LEFT JOIN` so an orphan fire (deleted
/// trigger — no cascade) still counts, bucketed as `"(deleted trigger)"`;
/// narrowed to `triggers`, it is left out.
async fn fires_by_pipeline(
    db: &sqlx::SqlitePool,
    from: &str,
    to: &str,
    triggers: Option<&BTreeSet<String>>,
) -> anyhow::Result<Vec<(String, i64)>> {
    let only = triggers_of(triggers);
    let narrow = if only.is_some() {
        format!(" AND f.trigger_id {ONLY_THESE}")
    } else {
        String::new()
    };
    let sql = format!(
        "SELECT COALESCE(t.pipeline_id, ?) AS pk, COUNT(*) AS count \
         FROM trigger_fires f LEFT JOIN triggers t ON f.trigger_id = t.id \
         WHERE f.ts >= ? AND f.ts < ?{narrow} \
         GROUP BY pk ORDER BY count DESC, pk"
    );
    let mut query = sqlx::query_as::<_, (String, i64)>(&sql)
        .bind(DELETED_TRIGGER)
        .bind(from)
        .bind(to);
    if let Some(only) = only {
        query = query.bind(only);
    }
    Ok(query.fetch_all(db).await?)
}

/// The "triggers that created a run" KPI: fired count, the enabled triggers that
/// fired, and the current enabled-trigger count — so « X of Y enabled triggers
/// fired » is a ratio: a trigger disabled since its fire is not in X (CORE-232).
async fn triggers_created_runs(
    db: &sqlx::SqlitePool,
    from: &str,
    to: &str,
    triggers: Option<&BTreeSet<String>>,
) -> anyhow::Result<TriggersCreatedRuns> {
    let only = triggers_of(triggers);
    let narrow = |column: &str| {
        if only.is_some() {
            format!(" AND {column} IN (SELECT value FROM json_each(?3))")
        } else {
            String::new()
        }
    };
    let sql = format!(
        "SELECT \
           (SELECT COUNT(*) FROM trigger_fires WHERE outcome = 'fired' AND ts >= ?1 AND ts < ?2{}) AS fired, \
           (SELECT COUNT(DISTINCT f.trigger_id) FROM trigger_fires f JOIN triggers t ON t.id = f.trigger_id \
            WHERE t.enabled = 1 AND f.outcome = 'fired' AND f.ts >= ?1 AND f.ts < ?2{}) AS distinct_triggers, \
           (SELECT COUNT(*) FROM triggers WHERE enabled = 1{}) AS enabled_triggers",
        narrow("trigger_id"),
        narrow("f.trigger_id"),
        narrow("id"),
    );
    let mut query = sqlx::query_as::<_, (i64, i64, i64)>(&sql)
        .bind(from)
        .bind(to);
    if let Some(only) = only {
        query = query.bind(only);
    }
    let (fired, distinct_triggers, enabled_triggers) = query.fetch_one(db).await?;
    Ok(TriggersCreatedRuns {
        fired,
        distinct_triggers,
        enabled_triggers,
    })
}

/// Every Trigger and the Projet it launches its Runs in.
async fn trigger_projects(db: &sqlx::SqlitePool) -> anyhow::Result<Vec<TriggerProject>> {
    Ok(
        sqlx::query_as::<_, TriggerProject>("SELECT id, project_id FROM triggers ORDER BY id")
            .fetch_all(db)
            .await?,
    )
}

impl TriggerFires for TriggerTableFires {
    fn by_pipeline<'a>(
        &'a self,
        from: &'a str,
        to: &'a str,
        triggers: Option<&'a BTreeSet<String>>,
    ) -> BoxFuture<'a, anyhow::Result<Vec<(String, i64)>>> {
        Box::pin(fires_by_pipeline(&self.db, from, to, triggers))
    }

    fn created_runs<'a>(
        &'a self,
        from: &'a str,
        to: &'a str,
        triggers: Option<&'a BTreeSet<String>>,
    ) -> BoxFuture<'a, anyhow::Result<TriggersCreatedRuns>> {
        Box::pin(triggers_created_runs(&self.db, from, to, triggers))
    }

    fn trigger_projects(&self) -> BoxFuture<'_, anyhow::Result<Vec<TriggerProject>>> {
        Box::pin(trigger_projects(&self.db))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FROM: &str = "2026-07-15T00:00:00Z";
    const TO: &str = "2026-07-18T00:00:00Z";

    #[tokio::test]
    async fn fires_left_join_surfaces_orphan_as_deleted_trigger() {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::daemon::storage::init_db(&db, std::path::Path::new("/"))
            .await
            .unwrap();
        // One live trigger + fires; plus a fire from a trigger that no longer exists.
        sqlx::query(
            "INSERT INTO triggers (id, name, pipeline_id, cron, enabled, created_at) \
             VALUES ('t1', 'nightly', 'alpha', '0 2 * * *', 1, '2026-07-14T00:00:00.000Z')",
        )
        .execute(&db)
        .await
        .unwrap();
        for (ts, outcome, run) in [
            ("2026-07-15T09:00:00.000Z", "fired", Some("r1")),
            ("2026-07-17T09:00:00.000Z", "fired", Some("r5")),
            ("2026-07-16T02:00:00.000Z", "skipped-overlap", None),
        ] {
            sqlx::query(
                "INSERT INTO trigger_fires (trigger_id, ts, outcome, run_id) VALUES ('t1', ?, ?, ?)",
            )
            .bind(ts)
            .bind(outcome)
            .bind(run)
            .execute(&db)
            .await
            .unwrap();
        }
        sqlx::query(
            "INSERT INTO trigger_fires (trigger_id, ts, outcome, run_id) \
             VALUES ('t-gone', '2026-07-16T05:00:00.000Z', 'fired', 'rX')",
        )
        .execute(&db)
        .await
        .unwrap();
        // A trigger disabled since it fired, and an enabled one that never fired.
        sqlx::query(
            "INSERT INTO triggers (id, name, pipeline_id, cron, enabled, created_at) VALUES \
             ('t2', 'paused', 'alpha', '0 3 * * *', 0, '2026-07-14T00:00:00.000Z'), \
             ('t3', 'idle', 'alpha', '0 4 * * *', 1, '2026-07-14T00:00:00.000Z')",
        )
        .execute(&db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO trigger_fires (trigger_id, ts, outcome, run_id) \
             VALUES ('t2', '2026-07-16T03:00:00.000Z', 'fired', 'rY')",
        )
        .execute(&db)
        .await
        .unwrap();

        let fires = TriggerTableFires { db: db.clone() };
        let by_pipeline = fires.by_pipeline(FROM, TO, None).await.unwrap();
        // alpha = 4 (t1: 2 fired + 1 skipped, t2: 1 fired), orphan = 1 under
        // "(deleted trigger)".
        assert_eq!(
            by_pipeline,
            vec![
                ("alpha".to_string(), 4),
                ("(deleted trigger)".to_string(), 1)
            ]
        );

        let created = fires.created_runs(FROM, TO, None).await.unwrap();
        // 4 fires with outcome='fired' (r1, r5, rX, rY). Of the 2 enabled triggers
        // (t1, t3), only t1 fired: the orphan and t2, disabled since, are not in the
        // ratio (CORE-232: « 2 of 2 » once read a disabled trigger as enabled).
        assert_eq!(created.fired, 4);
        assert_eq!(created.distinct_triggers, 1);
        assert_eq!(created.enabled_triggers, 2);

        // Narrowed to the Triggers a reader may read: t1 and t3 only. The
        // orphan fire and t2's are left out.
        let readable: BTreeSet<String> = ["t1".to_string(), "t3".to_string()].into();
        let by_pipeline = fires.by_pipeline(FROM, TO, Some(&readable)).await.unwrap();
        assert_eq!(by_pipeline, vec![("alpha".to_string(), 3)]);
        let created = fires.created_runs(FROM, TO, Some(&readable)).await.unwrap();
        assert_eq!(created.fired, 2);
        assert_eq!(created.distinct_triggers, 1);
        assert_eq!(created.enabled_triggers, 2);
        let none = BTreeSet::new();
        assert!(fires
            .by_pipeline(FROM, TO, Some(&none))
            .await
            .unwrap()
            .is_empty());
        let created = fires.created_runs(FROM, TO, Some(&none)).await.unwrap();
        assert_eq!(
            (
                created.fired,
                created.distinct_triggers,
                created.enabled_triggers
            ),
            (0, 0, 0)
        );
        let mut projects = fires.trigger_projects().await.unwrap();
        projects.sort();
        assert_eq!(
            projects,
            vec![
                ("t1".to_string(), None),
                ("t2".to_string(), None),
                ("t3".to_string(), None)
            ]
        );
    }
}
