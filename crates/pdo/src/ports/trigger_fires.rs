//! The port of the stats context that the Triggers answer: the fires of a
//! window (#377), read in the trigger context's tables (`pdo-trigger`, #927).
//! Neither context depends on the other, so the composition root reads one for
//! the other.

use pdo_stats::application::ports::{BoxFuture, TriggerFires};
use pdo_stats::domain::overview::fires::DELETED_TRIGGER;
use pdo_stats::domain::overview::TriggersCreatedRuns;

/// The Triggers of the daemon's database.
pub(crate) struct TriggerTableFires {
    pub(crate) db: sqlx::SqlitePool,
}

/// Fires per pipeline in the window. `LEFT JOIN` so an orphan fire (deleted
/// trigger — no cascade) still counts, bucketed as `"(deleted trigger)"`.
async fn fires_by_pipeline(
    db: &sqlx::SqlitePool,
    from: &str,
    to: &str,
) -> anyhow::Result<Vec<(String, i64)>> {
    Ok(sqlx::query_as::<_, (String, i64)>(
        "SELECT COALESCE(t.pipeline_id, ?) AS pk, COUNT(*) AS count \
         FROM trigger_fires f LEFT JOIN triggers t ON f.trigger_id = t.id \
         WHERE f.ts >= ? AND f.ts < ? \
         GROUP BY pk ORDER BY count DESC, pk",
    )
    .bind(DELETED_TRIGGER)
    .bind(from)
    .bind(to)
    .fetch_all(db)
    .await?)
}

/// The "triggers that created a run" KPI: fired count, the enabled triggers that
/// fired, and the current enabled-trigger count — so « X of Y enabled triggers
/// fired » is a ratio: a trigger disabled since its fire is not in X (CORE-232).
async fn triggers_created_runs(
    db: &sqlx::SqlitePool,
    from: &str,
    to: &str,
) -> anyhow::Result<TriggersCreatedRuns> {
    let (fired, distinct_triggers, enabled_triggers) = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT \
           (SELECT COUNT(*) FROM trigger_fires WHERE outcome = 'fired' AND ts >= ?1 AND ts < ?2) AS fired, \
           (SELECT COUNT(DISTINCT f.trigger_id) FROM trigger_fires f JOIN triggers t ON t.id = f.trigger_id \
            WHERE t.enabled = 1 AND f.outcome = 'fired' AND f.ts >= ?1 AND f.ts < ?2) AS distinct_triggers, \
           (SELECT COUNT(*) FROM triggers WHERE enabled = 1) AS enabled_triggers",
    )
    .bind(from)
    .bind(to)
    .fetch_one(db)
    .await?;
    Ok(TriggersCreatedRuns {
        fired,
        distinct_triggers,
        enabled_triggers,
    })
}

impl TriggerFires for TriggerTableFires {
    fn by_pipeline<'a>(
        &'a self,
        from: &'a str,
        to: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<Vec<(String, i64)>>> {
        Box::pin(fires_by_pipeline(&self.db, from, to))
    }

    fn created_runs<'a>(
        &'a self,
        from: &'a str,
        to: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<TriggersCreatedRuns>> {
        Box::pin(triggers_created_runs(&self.db, from, to))
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
        let by_pipeline = fires.by_pipeline(FROM, TO).await.unwrap();
        // alpha = 4 (t1: 2 fired + 1 skipped, t2: 1 fired), orphan = 1 under
        // "(deleted trigger)".
        assert_eq!(
            by_pipeline,
            vec![
                ("alpha".to_string(), 4),
                ("(deleted trigger)".to_string(), 1)
            ]
        );

        let created = fires.created_runs(FROM, TO).await.unwrap();
        // 4 fires with outcome='fired' (r1, r5, rX, rY). Of the 2 enabled triggers
        // (t1, t3), only t1 fired: the orphan and t2, disabled since, are not in the
        // ratio (CORE-232: « 2 of 2 » once read a disabled trigger as enabled).
        assert_eq!(created.fired, 4);
        assert_eq!(created.distinct_triggers, 1);
        assert_eq!(created.enabled_triggers, 2);
    }
}
