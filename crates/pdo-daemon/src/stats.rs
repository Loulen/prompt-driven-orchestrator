//! Instance-stats cockpit (#377, ADR-0029): cross-run, period-filterable
//! aggregates for the Stats modal. Two endpoints split by cost class:
//!
//! - [`stats_overview`] — **Class A**, cheap indexed SQL. Runs/errors/sessions
//!   per period (`GROUP BY strftime` over `events`, backed by
//!   `idx_events_kind_ts`), fires per pipeline (`LEFT JOIN triggers`, backed by
//!   `idx_trigger_fires_ts`), and the "triggers that created a run" KPI.
//! - [`stats_cost`] — **Class B**, heavy. Enumerates the `run_started` events in
//!   the period, resolves each run's estimated cost through the memoized
//!   [`crate::run_cost::compute_run_cost_cached`], and folds the per-run scalars
//!   app-side into by-period / by-pipeline / by-project buckets (cost has no
//!   pipeline/project dimension in SQL, and "by project" needs the
//!   `effective_repo_root` runtime fallback).
//!
//! Everything is derived on read — no snapshot table, no metric-freezing event
//! (preserves ADR-0022). Aggregated cost is a **sum of lower bounds**: partial
//! runs (an unpriced model) and null-cost runs (no transcript) are counted
//! separately so a bucket is never silently undercounted (ADR-0001 honesty).

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::event_log::CostStat;
use crate::AppState;

/// Query string shared by both endpoints: an ISO-8601 `[from, to)` window and a
/// bucket granularity. Mirrors the `State + Query` signature of
/// `list_reapable_runs`, but the body is indexed aggregate SQL, not per-run
/// replay.
#[derive(Debug, Deserialize)]
pub struct StatsQuery {
    /// Inclusive lower bound (ISO-8601, e.g. `2026-07-15T00:00:00Z`).
    pub from: String,
    /// Exclusive upper bound.
    pub to: String,
    /// `day` | `week` | `month`.
    pub bucket: String,
}

/// Map a bucket granularity to its SQLite `strftime` format. `None` for an
/// unknown granularity (the handler answers `400`).
fn strftime_fmt(bucket: &str) -> Option<&'static str> {
    match bucket {
        "day" => Some("%Y-%m-%d"),
        "week" => Some("%Y-W%W"),
        "month" => Some("%Y-%m"),
        _ => None,
    }
}

// --- Overview (Class A) ------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BucketCount {
    pub bucket: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PipelineFireCount {
    /// The trigger's `pipeline_id`, or `"(deleted trigger)"` for an orphan fire
    /// (the trigger row was deleted; there is no cascade, so the fire survives
    /// and must be surfaced, never dropped — hence the `LEFT JOIN`).
    pub pipeline_id: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TriggersCreatedRuns {
    /// Fires whose `outcome = 'fired'` (⟺ a run was created) in the window.
    pub fired: i64,
    /// Distinct triggers that fired at least once in the window.
    pub distinct_triggers: i64,
    /// Triggers currently `enabled` (a point-in-time count, not windowed).
    pub enabled_triggers: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatsOverview {
    /// Sorted union of period labels across runs/errors/sessions — the ordered
    /// x-axis the client renders against.
    pub buckets: Vec<String>,
    pub runs: Vec<BucketCount>,
    pub errors: Vec<BucketCount>,
    pub sessions: Vec<BucketCount>,
    pub fires_by_pipeline: Vec<PipelineFireCount>,
    pub triggers_created_runs: TriggersCreatedRuns,
}

/// Count events of one `kind` per period bucket. Backed by `idx_events_kind_ts`.
async fn count_events_by_bucket(
    db: &sqlx::SqlitePool,
    fmt: &str,
    kind: &str,
    from: &str,
    to: &str,
) -> Result<Vec<BucketCount>, sqlx::Error> {
    let rows = sqlx::query_as::<_, (String, i64)>(
        "SELECT strftime(?, ts) AS bucket, COUNT(*) AS count \
         FROM events WHERE kind = ? AND ts >= ? AND ts < ? \
         GROUP BY bucket ORDER BY bucket",
    )
    .bind(fmt)
    .bind(kind)
    .bind(from)
    .bind(to)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(bucket, count)| BucketCount { bucket, count })
        .collect())
}

/// Fires per pipeline in the window. `LEFT JOIN` so an orphan fire (deleted
/// trigger — no cascade) still counts, bucketed as `"(deleted trigger)"`.
async fn fires_by_pipeline(
    db: &sqlx::SqlitePool,
    from: &str,
    to: &str,
) -> Result<Vec<PipelineFireCount>, sqlx::Error> {
    let rows = sqlx::query_as::<_, (String, i64)>(
        "SELECT COALESCE(t.pipeline_id, '(deleted trigger)') AS pk, COUNT(*) AS count \
         FROM trigger_fires f LEFT JOIN triggers t ON f.trigger_id = t.id \
         WHERE f.ts >= ? AND f.ts < ? \
         GROUP BY pk ORDER BY count DESC, pk",
    )
    .bind(from)
    .bind(to)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(pipeline_id, count)| PipelineFireCount { pipeline_id, count })
        .collect())
}

/// The "triggers that created a run" KPI: fired count, distinct fired triggers,
/// and the current enabled-trigger count.
async fn triggers_created_runs(
    db: &sqlx::SqlitePool,
    from: &str,
    to: &str,
) -> Result<TriggersCreatedRuns, sqlx::Error> {
    let (fired, distinct_triggers, enabled_triggers) = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT \
           (SELECT COUNT(*) FROM trigger_fires WHERE outcome = 'fired' AND ts >= ?1 AND ts < ?2) AS fired, \
           (SELECT COUNT(DISTINCT trigger_id) FROM trigger_fires WHERE outcome = 'fired' AND ts >= ?1 AND ts < ?2) AS distinct_triggers, \
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

/// Assemble the full overview payload (testable without an `AppState`).
async fn compute_overview(
    db: &sqlx::SqlitePool,
    fmt: &str,
    from: &str,
    to: &str,
) -> Result<StatsOverview, sqlx::Error> {
    // `run_skipped` is NOT an error (invariant #4): errors = `run_failed` only.
    let runs = count_events_by_bucket(db, fmt, "run_started", from, to).await?;
    let errors = count_events_by_bucket(db, fmt, "run_failed", from, to).await?;
    // Sessions = `node_started` starts (re-spawns and loop laps included, manager
    // excluded by construction) — the same cumulative count as the per-run stat.
    let sessions = count_events_by_bucket(db, fmt, "node_started", from, to).await?;
    let fires = fires_by_pipeline(db, from, to).await?;
    let created = triggers_created_runs(db, from, to).await?;

    let mut labels: BTreeSet<String> = BTreeSet::new();
    for series in [&runs, &errors, &sessions] {
        for row in series {
            labels.insert(row.bucket.clone());
        }
    }

    Ok(StatsOverview {
        buckets: labels.into_iter().collect(),
        runs,
        errors,
        sessions,
        fires_by_pipeline: fires,
        triggers_created_runs: created,
    })
}

/// `GET /stats/overview` — Class A cheap indexed SQL.
pub async fn stats_overview(
    State(state): State<Arc<AppState>>,
    Query(q): Query<StatsQuery>,
) -> Response {
    let Some(fmt) = strftime_fmt(&q.bucket) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("invalid bucket: {}", q.bucket) })),
        )
            .into_response();
    };
    match compute_overview(&state.db, fmt, &q.from, &q.to).await {
        Ok(overview) => Json(overview).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("stats overview failed: {e}") })),
        )
            .into_response(),
    }
}

// --- Cost (Class B) ----------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CostPeriodBucket {
    pub bucket: String,
    /// Sum of priced per-run costs (a lower bound — see `partial`/`null`).
    pub usd: f64,
    /// Runs in this bucket whose cost is a lower bound (an unpriced model).
    pub partial: i64,
    /// Runs in this bucket with no transcript (excluded from `usd`, surfaced so
    /// the bucket is never silently undercounted). Serialized as `"null"`.
    #[serde(rename = "null")]
    pub null_count: i64,
    /// Total runs folded into this bucket (priced + partial + null).
    pub runs: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CostKeyBucket {
    pub key: String,
    pub usd: f64,
    pub partial: i64,
    #[serde(rename = "null")]
    pub null_count: i64,
    pub runs: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatsCost {
    pub by_period: Vec<CostPeriodBucket>,
    pub by_pipeline: Vec<CostKeyBucket>,
    pub by_project: Vec<CostKeyBucket>,
}

/// One run's contribution to the cost fold: its period bucket, pipeline key,
/// project key, and resolved cost (`None` = no transcript).
struct CostRow {
    bucket: String,
    pipeline: String,
    project: String,
    cost: Option<CostStat>,
}

#[derive(Default, Clone)]
struct CostAcc {
    usd: f64,
    partial: i64,
    null_count: i64,
    runs: i64,
}

impl CostAcc {
    fn add(&mut self, cost: &Option<CostStat>) {
        self.runs += 1;
        match cost {
            Some(c) => {
                self.usd += c.usd;
                if c.partial {
                    self.partial += 1;
                }
            }
            None => self.null_count += 1,
        }
    }
}

/// Fold per-run cost rows into the three categorical breakdowns. Pure — the
/// handler does the DB enumeration and cost resolution, this only accumulates,
/// so it is unit-testable with synthetic rows.
fn fold_cost(rows: &[CostRow]) -> StatsCost {
    use std::collections::HashMap;
    let mut by_period: HashMap<&str, CostAcc> = HashMap::new();
    let mut by_pipeline: HashMap<&str, CostAcc> = HashMap::new();
    let mut by_project: HashMap<&str, CostAcc> = HashMap::new();
    for r in rows {
        by_period.entry(&r.bucket).or_default().add(&r.cost);
        by_pipeline.entry(&r.pipeline).or_default().add(&r.cost);
        by_project.entry(&r.project).or_default().add(&r.cost);
    }

    // by_period sorts by label (chronological); the categorical axes sort by
    // spend desc, then key, for a stable, meaningful order.
    let mut period: Vec<CostPeriodBucket> = by_period
        .into_iter()
        .map(|(bucket, a)| CostPeriodBucket {
            bucket: bucket.to_string(),
            usd: a.usd,
            partial: a.partial,
            null_count: a.null_count,
            runs: a.runs,
        })
        .collect();
    period.sort_by(|a, b| a.bucket.cmp(&b.bucket));

    let to_key_buckets = |map: HashMap<&str, CostAcc>| {
        let mut v: Vec<CostKeyBucket> = map
            .into_iter()
            .map(|(key, a)| CostKeyBucket {
                key: key.to_string(),
                usd: a.usd,
                partial: a.partial,
                null_count: a.null_count,
                runs: a.runs,
            })
            .collect();
        v.sort_by(|a, b| {
            b.usd
                .partial_cmp(&a.usd)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.key.cmp(&b.key))
        });
        v
    };

    StatsCost {
        by_period: period,
        by_pipeline: to_key_buckets(by_pipeline),
        by_project: to_key_buckets(by_project),
    }
}

/// `GET /stats/cost` — Class B, memo + app-side fold. Heavy (fans over the
/// `~/.claude` corpus); fetched lazily by the client only when the cost tab is
/// shown.
pub async fn stats_cost(
    State(state): State<Arc<AppState>>,
    Query(q): Query<StatsQuery>,
) -> Response {
    let Some(fmt) = strftime_fmt(&q.bucket) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("invalid bucket: {}", q.bucket) })),
        )
            .into_response();
    };

    // Cheap SQL: every `run_started` in the window, with its period bucket
    // (identical strftime to the overview endpoint) and payload.
    let rows = match sqlx::query_as::<_, (String, String, Option<String>)>(
        "SELECT run_id, strftime(?, ts) AS bucket, payload \
         FROM events WHERE kind = 'run_started' AND ts >= ? AND ts < ? ORDER BY ts",
    )
    .bind(fmt)
    .bind(&q.from)
    .bind(&q.to)
    .fetch_all(&state.db)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("stats cost failed: {e}") })),
            )
                .into_response();
        }
    };

    let mut cost_rows: Vec<CostRow> = Vec::with_capacity(rows.len());
    for (run_id, bucket, payload) in rows {
        let payload: serde_json::Value = payload
            .as_deref()
            .and_then(|p| serde_json::from_str(p).ok())
            .unwrap_or(serde_json::Value::Null);

        // Pipeline key: `pipeline_id` going forward (#377), else the (always
        // present) `pipeline_name` — so grouping survives a rename (#230).
        let pipeline = payload
            .get("pipeline_id")
            .and_then(|v| v.as_str())
            .or_else(|| payload.get("pipeline_name").and_then(|v| v.as_str()))
            .unwrap_or("(unknown)")
            .to_string();

        // "By project" = `effective_repo_root`: the run's `target_repo`, else the
        // daemon repo root. No "Unassigned" bucket (invariant #6, #258).
        let target_repo = payload.get("target_repo").and_then(|v| v.as_str());
        let repo_root: PathBuf = target_repo
            .map(PathBuf::from)
            .unwrap_or_else(|| state.repo_root.clone());
        let project = repo_root.to_string_lossy().into_owned();

        let cost = crate::run_cost::compute_run_cost_cached(&repo_root, &run_id);
        cost_rows.push(CostRow {
            bucket,
            pipeline,
            project,
            cost,
        });
    }

    Json(fold_cost(&cost_rows)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn mem_db() -> sqlx::SqlitePool {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::init_db(&db).await.unwrap();
        db
    }

    /// Insert a `run_started` + a terminal event for a run.
    async fn seed_run(
        db: &sqlx::SqlitePool,
        run_id: &str,
        pipeline_name: &str,
        target_repo: &str,
        day: &str,
        terminal: &str,
    ) {
        let payload =
            serde_json::json!({ "pipeline_name": pipeline_name, "target_repo": target_repo })
                .to_string();
        sqlx::query(
            "INSERT INTO events (run_id, ts, kind, payload) VALUES (?, ?, 'run_started', ?)",
        )
        .bind(run_id)
        .bind(format!("{day}T09:00:00.000Z"))
        .bind(&payload)
        .execute(db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO events (run_id, ts, kind, payload) VALUES (?, ?, ?, NULL)")
            .bind(run_id)
            .bind(format!("{day}T09:05:00.000Z"))
            .bind(terminal)
            .execute(db)
            .await
            .unwrap();
    }

    async fn seed_session(db: &sqlx::SqlitePool, run_id: &str, day: &str) {
        sqlx::query(
            "INSERT INTO events (run_id, ts, kind, node_id, iter) VALUES (?, ?, 'node_started', 'doer', 0)",
        )
        .bind(run_id)
        .bind(format!("{day}T09:01:00.000Z"))
        .execute(db)
        .await
        .unwrap();
    }

    /// The FP-377 oracle fixture (6 runs across three days).
    async fn seed_oracle(db: &sqlx::SqlitePool) {
        seed_run(db, "r1", "alpha", "/proj/A", "2026-07-15", "run_completed").await;
        seed_run(db, "r2", "alpha", "/proj/A", "2026-07-15", "run_failed").await;
        seed_run(db, "r3", "beta", "/proj/B", "2026-07-16", "run_completed").await;
        seed_run(db, "r4", "beta", "/proj/B", "2026-07-16", "run_completed").await;
        seed_run(db, "r5", "alpha", "/proj/A", "2026-07-17", "run_skipped").await;
        seed_run(db, "r6", "beta", "/proj/B", "2026-07-17", "run_failed").await;
        seed_session(db, "r1", "2026-07-15").await;
        seed_session(db, "r3", "2026-07-16").await;
        seed_session(db, "r4", "2026-07-16").await;
    }

    const FROM: &str = "2026-07-15T00:00:00Z";
    const TO: &str = "2026-07-18T00:00:00Z";

    #[tokio::test]
    async fn overview_runs_errors_sessions_per_day() {
        let db = mem_db().await;
        seed_oracle(&db).await;
        let ov = compute_overview(&db, "%Y-%m-%d", FROM, TO).await.unwrap();

        assert_eq!(
            ov.runs,
            vec![
                BucketCount {
                    bucket: "2026-07-15".into(),
                    count: 2
                },
                BucketCount {
                    bucket: "2026-07-16".into(),
                    count: 2
                },
                BucketCount {
                    bucket: "2026-07-17".into(),
                    count: 2
                },
            ]
        );
        // Errors = run_failed only; run_skipped (r5) is NOT an error.
        assert_eq!(
            ov.errors,
            vec![
                BucketCount {
                    bucket: "2026-07-15".into(),
                    count: 1
                },
                BucketCount {
                    bucket: "2026-07-17".into(),
                    count: 1
                },
            ]
        );
        let total_errors: i64 = ov.errors.iter().map(|b| b.count).sum();
        assert_eq!(total_errors, 2, "run_skipped must not inflate errors");
        // Sessions = node_started, cumulative = 3.
        let total_sessions: i64 = ov.sessions.iter().map(|b| b.count).sum();
        assert_eq!(total_sessions, 3);
        // buckets = sorted union across the three series.
        assert_eq!(ov.buckets, vec!["2026-07-15", "2026-07-16", "2026-07-17"]);
    }

    #[tokio::test]
    async fn overview_period_bounds_are_half_open() {
        let db = mem_db().await;
        seed_oracle(&db).await;
        // A window that ends exactly at the 17th 00:00 excludes the 17th's runs.
        let ov = compute_overview(&db, "%Y-%m-%d", FROM, "2026-07-17T00:00:00Z")
            .await
            .unwrap();
        let total_runs: i64 = ov.runs.iter().map(|b| b.count).sum();
        assert_eq!(total_runs, 4, "half-open [from, to): the 17th is excluded");
    }

    #[tokio::test]
    async fn fires_left_join_surfaces_orphan_as_deleted_trigger() {
        let db = mem_db().await;
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
        // Orphan fire: trigger 't-gone' has no row in `triggers`.
        sqlx::query(
            "INSERT INTO trigger_fires (trigger_id, ts, outcome, run_id) \
             VALUES ('t-gone', '2026-07-16T05:00:00.000Z', 'fired', 'rX')",
        )
        .execute(&db)
        .await
        .unwrap();

        let fires = fires_by_pipeline(&db, FROM, TO).await.unwrap();
        // alpha = 3 (2 fired + 1 skipped), orphan = 1 under "(deleted trigger)".
        let alpha = fires.iter().find(|f| f.pipeline_id == "alpha").unwrap();
        assert_eq!(alpha.count, 3);
        let orphan = fires
            .iter()
            .find(|f| f.pipeline_id == "(deleted trigger)")
            .unwrap();
        assert_eq!(orphan.count, 1);

        let created = triggers_created_runs(&db, FROM, TO).await.unwrap();
        // 3 fires with outcome='fired' (r1, r5, rX), from 1 distinct existing +
        // 1 orphan = 2 distinct trigger_ids; 1 enabled trigger.
        assert_eq!(created.fired, 3);
        assert_eq!(created.distinct_triggers, 2);
        assert_eq!(created.enabled_triggers, 1);
    }

    #[test]
    fn fold_cost_sums_and_propagates_partial_and_null() {
        let rows = vec![
            CostRow {
                bucket: "2026-07-15".into(),
                pipeline: "alpha".into(),
                project: "/proj/A".into(),
                cost: Some(CostStat {
                    usd: 1.0,
                    partial: false,
                }),
            },
            CostRow {
                bucket: "2026-07-15".into(),
                pipeline: "alpha".into(),
                project: "/proj/A".into(),
                cost: Some(CostStat {
                    usd: 2.0,
                    partial: true,
                }),
            },
            CostRow {
                bucket: "2026-07-16".into(),
                pipeline: "beta".into(),
                project: "/proj/B".into(),
                cost: None, // no transcript
            },
        ];
        let c = fold_cost(&rows);

        // by_period: the 15th sums 1.0 + 2.0 with one partial run; the 16th is
        // all-null (usd 0, null 1).
        let d15 = c
            .by_period
            .iter()
            .find(|b| b.bucket == "2026-07-15")
            .unwrap();
        assert!((d15.usd - 3.0).abs() < 1e-9);
        assert_eq!(d15.partial, 1);
        assert_eq!(d15.null_count, 0);
        assert_eq!(d15.runs, 2);
        let d16 = c
            .by_period
            .iter()
            .find(|b| b.bucket == "2026-07-16")
            .unwrap();
        assert_eq!(d16.usd, 0.0);
        assert_eq!(d16.null_count, 1);
        assert_eq!(d16.runs, 1);
        // by_period is chronological.
        assert_eq!(
            c.by_period
                .iter()
                .map(|b| b.bucket.as_str())
                .collect::<Vec<_>>(),
            vec!["2026-07-15", "2026-07-16"]
        );

        // by_pipeline: alpha carries all the spend (sorted first, desc by usd).
        assert_eq!(c.by_pipeline[0].key, "alpha");
        assert!((c.by_pipeline[0].usd - 3.0).abs() < 1e-9);
        assert_eq!(c.by_pipeline[0].partial, 1);
        assert_eq!(c.by_pipeline[0].runs, 2);
        let beta = c.by_pipeline.iter().find(|b| b.key == "beta").unwrap();
        assert_eq!(beta.usd, 0.0);
        assert_eq!(beta.null_count, 1);

        // by_project mirrors the pipeline split here.
        assert_eq!(c.by_project[0].key, "/proj/A");
        assert!((c.by_project[0].usd - 3.0).abs() < 1e-9);
    }

    #[test]
    fn strftime_fmt_maps_known_buckets_only() {
        assert_eq!(strftime_fmt("day"), Some("%Y-%m-%d"));
        assert_eq!(strftime_fmt("week"), Some("%Y-W%W"));
        assert_eq!(strftime_fmt("month"), Some("%Y-%m"));
        assert_eq!(strftime_fmt("year"), None);
        assert_eq!(strftime_fmt(""), None);
    }
}
