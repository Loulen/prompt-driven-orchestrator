//! Instance-stats cockpit (#377, ADR-0029): cross-run, period-filterable
//! aggregates for the Stats modal. Two endpoints split by cost class:
//!
//! - [`stats_overview`] keeps the indexed Runs and Triggers queries, then selects
//!   the session cohort by `run_started.ts`. Every agentic `node_started` in a
//!   selected Run counts, including later loop laps and restarts. The response
//!   splits those executions by dynamic harness and Pipeline → Node identity.
//! - [`stats_cost`] is the lazy heavy read. It selects the same Run cohort,
//!   resolves memoized harness-specific contributions, and folds them into
//!   period, Pipeline → Node, and Project → Pipeline → Node hierarchies. The
//!   response includes readable denominators, unknown-cost reasons, and the
//!   resolved price table used for derived Claude costs.
//!
//! Everything is derived on read — no snapshot table, no metric-freezing event
//! (preserves ADR-0022). Aggregated cost is a **sum of lower bounds**: partial
//! runs (an unpriced model) and null-cost runs (no transcript) are counted
//! separately so a bucket is never silently undercounted (ADR-0001 honesty).

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::AppState;

/// Query string shared by both endpoints: an ISO-8601 `[from, to)` window and a
/// bucket granularity. Mirrors the `State + Query` signature of
/// `list_reapable_runs`, but the body is indexed aggregate SQL, not per-run
/// replay.
#[derive(Debug, Deserialize)]
pub(crate) struct StatsQuery {
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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct BucketCount {
    pub bucket: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct PipelineFireCount {
    /// The trigger's `pipeline_id`, or `"(deleted trigger)"` for an orphan fire
    /// (the trigger row was deleted; there is no cascade, so the fire survives
    /// and must be surfaced, never dropped — hence the `LEFT JOIN`).
    pub pipeline_id: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct TriggersCreatedRuns {
    /// Fires whose `outcome = 'fired'` (⟺ a run was created) in the window.
    pub fired: i64,
    /// Distinct triggers that fired at least once in the window.
    pub distinct_triggers: i64,
    /// Triggers currently `enabled` (a point-in-time count, not windowed).
    pub enabled_triggers: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct StatsOverview {
    /// Sorted union of period labels across runs/errors/sessions — the ordered
    /// x-axis the client renders against.
    pub buckets: Vec<String>,
    pub runs: Vec<BucketCount>,
    pub errors: Vec<BucketCount>,
    pub sessions: Vec<BucketCount>,
    pub fires_by_pipeline: Vec<PipelineFireCount>,
    pub triggers_created_runs: TriggersCreatedRuns,
    pub session_harnesses: Vec<String>,
    pub sessions_by_period: Vec<StatsSessionPeriod>,
    pub sessions_by_pipeline: Vec<StatsSessionEntity>,
}

#[derive(Debug, Clone, Default)]
struct HarnessCount {
    pub total: u64,
    pub by_harness: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct StatsSessionHarness {
    pub harness: String,
    pub executions: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct StatsSessionPeriod {
    pub bucket: String,
    pub harnesses: Vec<StatsSessionHarness>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct StatsSessionEntity {
    pub id: String,
    pub name: String,
    pub executions: u64,
    pub harnesses: Vec<StatsSessionHarness>,
    pub by_period: Vec<StatsSessionPeriod>,
    pub nodes: Vec<StatsSessionEntity>,
}

#[derive(Debug, Clone, Default)]
struct SessionNodeBuilder {
    name: String,
    count: HarnessCount,
    periods: BTreeMap<String, HarnessCount>,
}

#[derive(Debug, Clone, Default)]
struct SessionPipelineBuilder {
    name: String,
    count: HarnessCount,
    periods: BTreeMap<String, HarnessCount>,
    nodes: BTreeMap<String, SessionNodeBuilder>,
}

fn increment_harness(count: &mut HarnessCount, harness: &str) {
    count.total += 1;
    *count.by_harness.entry(harness.to_string()).or_default() += 1;
}

fn harness_rows(count: BTreeMap<String, u64>) -> Vec<StatsSessionHarness> {
    count
        .into_iter()
        .map(|(harness, executions)| StatsSessionHarness {
            harness,
            executions,
        })
        .collect()
}

fn period_rows(periods: BTreeMap<String, HarnessCount>) -> Vec<StatsSessionPeriod> {
    periods
        .into_iter()
        .map(|(bucket, count)| StatsSessionPeriod {
            bucket,
            harnesses: harness_rows(count.by_harness),
        })
        .collect()
}

fn finish_node(id: String, builder: SessionNodeBuilder) -> StatsSessionEntity {
    StatsSessionEntity {
        id,
        name: builder.name,
        executions: builder.count.total,
        harnesses: harness_rows(builder.count.by_harness),
        by_period: period_rows(builder.periods),
        nodes: Vec::new(),
    }
}

fn finish_pipeline(id: String, builder: SessionPipelineBuilder) -> StatsSessionEntity {
    let mut nodes: Vec<StatsSessionEntity> = builder
        .nodes
        .into_iter()
        .map(|(id, node)| finish_node(id, node))
        .collect();
    nodes.sort_by(|a, b| {
        b.executions
            .cmp(&a.executions)
            .then_with(|| a.id.cmp(&b.id))
    });
    StatsSessionEntity {
        id,
        name: builder.name,
        executions: builder.count.total,
        harnesses: harness_rows(builder.count.by_harness),
        by_period: period_rows(builder.periods),
        nodes,
    }
}

fn project_identity(
    payload: &serde_json::Value,
    daemon_root: &Path,
    projects: &[crate::project_store::Project],
) -> (String, String) {
    let root = cost_project_root(payload, daemon_root);
    let root_text = root.to_string_lossy().into_owned();
    if let Some(project) = projects
        .iter()
        .find(|project| project.members.iter().any(|member| member == &root_text))
    {
        return (project.id.clone(), project.name.clone());
    }
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(&root_text)
        .to_string();
    (root_text, name)
}

struct SessionStats {
    sessions: Vec<BucketCount>,
    periods: Vec<StatsSessionPeriod>,
    harnesses: Vec<String>,
    pipelines: Vec<StatsSessionEntity>,
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

async fn session_stats(
    state: &AppState,
    q: &StatsQuery,
    fmt: &str,
) -> Result<SessionStats, sqlx::Error> {
    type SessionRow = (
        String,
        String,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<i64>,
        Option<String>,
    );
    let rows = sqlx::query_as::<_, SessionRow>(
        "SELECT strftime(?, cohort.ts), cohort.run_id, cohort.payload, event.id, \
                event.node_id, event.iter, event.payload \
         FROM events cohort \
         JOIN events event ON event.run_id = cohort.run_id AND event.kind = 'node_started' \
         WHERE cohort.kind = 'run_started' AND cohort.ts >= ? AND cohort.ts < ? \
         ORDER BY cohort.ts, event.id",
    )
    .bind(fmt)
    .bind(&q.from)
    .bind(&q.to)
    .fetch_all(&state.db)
    .await?;

    let mut periods = BTreeMap::<String, HarnessCount>::new();
    let mut session_periods = BTreeMap::<String, i64>::new();
    let mut harnesses = BTreeSet::<String>::new();
    let mut pipelines = BTreeMap::<String, SessionPipelineBuilder>::new();
    let mut seen_executions = HashSet::new();

    for (order, (bucket, run_id, run_payload, event_id, node_id, iter, event_payload)) in
        rows.into_iter().enumerate()
    {
        let run_payload: serde_json::Value = run_payload
            .as_deref()
            .and_then(|payload| serde_json::from_str(payload).ok())
            .unwrap_or(serde_json::Value::Null);
        let event_payload: serde_json::Value = event_payload
            .as_deref()
            .and_then(|payload| serde_json::from_str(payload).ok())
            .unwrap_or(serde_json::Value::Null);
        let pipeline_id = run_payload
            .get("pipeline_id")
            .and_then(|value| value.as_str())
            .or_else(|| {
                run_payload
                    .get("pipeline_name")
                    .and_then(|value| value.as_str())
            })
            .unwrap_or("(unknown)")
            .to_string();
        let pipeline_name = run_payload
            .get("pipeline_name")
            .and_then(|value| value.as_str())
            .unwrap_or(&pipeline_id)
            .to_string();
        let mut node_defs = BTreeMap::<String, (String, String)>::new();
        if let Some(defs) = run_payload
            .get("node_defs")
            .and_then(|value| value.as_array())
        {
            for def in defs {
                let Some(id) = def.get("id").and_then(|value| value.as_str()) else {
                    continue;
                };
                let name = def
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or(id)
                    .to_string();
                let node_type = def
                    .get("node_type")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .to_string();
                node_defs.insert(id.to_string(), (name, node_type));
            }
        }

        let Some(node_id) = node_id else {
            continue;
        };
        let node_type = event_payload
            .get("node_type")
            .and_then(|value| value.as_str())
            .or_else(|| node_defs.get(&node_id).map(|(_, kind)| kind.as_str()))
            .unwrap_or("");
        if node_type == "script" {
            continue;
        }
        let harness = event_payload
            .get("harness")
            .and_then(|value| value.as_str())
            .filter(|harness| !harness.is_empty())
            .unwrap_or("claude")
            .to_string();
        let identity = crate::run_cost::frozen_execution_identity(
            &harness,
            event_payload
                .get("session_id")
                .and_then(|value| value.as_str()),
            Some(&node_id),
            iter,
            event_id,
            order,
        );
        if !seen_executions.insert((run_id, identity)) {
            continue;
        }

        harnesses.insert(harness.clone());
        increment_harness(periods.entry(bucket.clone()).or_default(), &harness);

        let pipeline = pipelines.entry(pipeline_id.clone()).or_default();
        pipeline.name = pipeline_name.clone();
        increment_harness(&mut pipeline.count, &harness);
        increment_harness(
            pipeline.periods.entry(bucket.clone()).or_default(),
            &harness,
        );

        *session_periods.entry(bucket.clone()).or_default() += 1;
        let node_name = node_defs
            .get(&node_id)
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| node_id.clone());
        let node = pipeline.nodes.entry(node_id.clone()).or_default();
        node.name = node_name;
        increment_harness(&mut node.count, &harness);
        increment_harness(node.periods.entry(bucket).or_default(), &harness);
    }

    let mut pipeline_rows: Vec<_> = pipelines
        .into_iter()
        .map(|(id, pipeline)| finish_pipeline(id, pipeline))
        .collect();
    pipeline_rows.sort_by(|a, b| {
        b.executions
            .cmp(&a.executions)
            .then_with(|| a.id.cmp(&b.id))
    });

    Ok(SessionStats {
        sessions: session_periods
            .into_iter()
            .map(|(bucket, count)| BucketCount { bucket, count })
            .collect(),
        periods: period_rows(periods),
        harnesses: harnesses.into_iter().collect(),
        pipelines: pipeline_rows,
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
        session_harnesses: Vec::new(),
        sessions_by_period: Vec::new(),
        sessions_by_pipeline: Vec::new(),
    })
}

/// `GET /stats/overview` — Class A cheap indexed SQL.
pub(crate) async fn stats_overview(
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
    match (
        compute_overview(&state.db, fmt, &q.from, &q.to).await,
        session_stats(&state, &q, fmt).await,
    ) {
        (Ok(mut overview), Ok(sessions)) => {
            overview.sessions = sessions.sessions;
            overview.session_harnesses = sessions.harnesses;
            overview.sessions_by_period = sessions.periods;
            overview.sessions_by_pipeline = sessions.pipelines;
            Json(overview).into_response()
        }
        (Err(e), _) | (_, Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("stats overview failed: {e}") })),
        )
            .into_response(),
    }
}

/// One resolved price row (#528): a family key, the tier that decides it, and the
/// `$/MTok` actually in force. `tier` serializes as `"manual" | "fetched" |
/// "embedded"` (the `PriceTier` `rename_all = "lowercase"`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct ResolvedPriceRow {
    pub key: String,
    pub tier: crate::price_table::PriceTier,
    /// $/MTok in — the price ACTUALLY applied (the winning tier).
    pub input: f64,
    /// $/MTok out — the price ACTUALLY applied (the winning tier).
    pub output: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct StatsHarnessCost {
    pub harness: String,
    pub usd: Option<f64>,
    pub estimated: bool,
    pub partial: bool,
    pub executions: i64,
    pub readable: i64,
    pub unknown: i64,
    pub average_usd: Option<f64>,
    pub unpriced_models: Vec<String>,
    pub missing_reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct StatsCostAggregate {
    pub usd: Option<f64>,
    pub average_usd: Option<f64>,
    pub estimated: bool,
    pub partial: bool,
    pub executions: i64,
    pub readable: i64,
    pub unknown: i64,
    pub unpriced_models: Vec<String>,
    pub missing_reasons: Vec<String>,
    pub harnesses: Vec<StatsHarnessCost>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct StatsCostPeriod {
    pub bucket: String,
    #[serde(flatten)]
    pub aggregate: StatsCostAggregate,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct StatsCostEntity {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub aggregate: StatsCostAggregate,
    pub by_period: Vec<StatsCostPeriod>,
    pub nodes: Vec<StatsCostEntity>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct StatsProjectCostEntity {
    #[serde(flatten)]
    pub entity: StatsCostEntity,
    pub pipelines: Vec<StatsCostEntity>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct StatsCost {
    pub harnesses: Vec<String>,
    pub total: StatsCostAggregate,
    pub by_period: Vec<StatsCostPeriod>,
    pub by_pipeline: Vec<StatsCostEntity>,
    pub by_project: Vec<StatsProjectCostEntity>,
    pub resolved: Vec<ResolvedPriceRow>,
}

#[derive(Debug, Clone, Default)]
struct CostMetricAcc {
    usd: f64,
    readable_usd: f64,
    has_usd: bool,
    estimated: bool,
    partial: bool,
    executions: i64,
    readable: i64,
    unknown: i64,
    unpriced_models: BTreeSet<String>,
    missing_reasons: BTreeSet<String>,
}

impl CostMetricAcc {
    fn add_contribution(&mut self, contribution: &crate::run_cost::CostContribution) {
        self.executions += contribution.executions;
        self.readable += contribution.readable_executions;
        self.unknown += contribution.executions - contribution.readable_executions;
        if let Some(usd) = contribution.usd {
            self.usd += usd;
            self.has_usd = true;
            if contribution.readable_executions > 0 {
                self.readable_usd += usd;
            }
        }
        self.estimated |= contribution.form == Some(crate::event_log::CostForm::Derived);
        self.partial |= contribution.partial;
        self.unpriced_models
            .extend(contribution.unpriced_models.iter().cloned());
        self.missing_reasons
            .extend(contribution.unavailable_reasons.iter().cloned());
    }

    fn add_run(&mut self, contributions: &[&crate::run_cost::CostContribution]) {
        if contributions.is_empty() {
            return;
        }
        self.executions += 1;
        let mut all_readable = true;
        let mut run_usd = 0.0;
        for contribution in contributions {
            if let Some(usd) = contribution.usd {
                self.usd += usd;
                run_usd += usd;
                self.has_usd = true;
            }
            all_readable &= if contribution.executions > 0 {
                contribution.readable_executions == contribution.executions
            } else {
                contribution.usd.is_some()
            };
            self.estimated |= contribution.form == Some(crate::event_log::CostForm::Derived);
            self.partial |= contribution.partial;
            self.unpriced_models
                .extend(contribution.unpriced_models.iter().cloned());
            self.missing_reasons
                .extend(contribution.unavailable_reasons.iter().cloned());
        }
        if all_readable {
            self.readable_usd += run_usd;
        }
        self.readable += i64::from(all_readable);
        self.unknown += i64::from(!all_readable);
    }

    fn wire(&self) -> StatsCostAggregate {
        StatsCostAggregate {
            usd: self.has_usd.then_some(self.usd),
            average_usd: (self.readable > 0).then_some(self.readable_usd / self.readable as f64),
            estimated: self.estimated,
            partial: self.partial,
            executions: self.executions,
            readable: self.readable,
            unknown: self.unknown.max(0),
            unpriced_models: self.unpriced_models.iter().cloned().collect(),
            missing_reasons: self.missing_reasons.iter().cloned().collect(),
            harnesses: Vec::new(),
        }
    }

    fn wire_harness(&self, harness: String) -> StatsHarnessCost {
        StatsHarnessCost {
            harness,
            usd: self.has_usd.then_some(self.usd),
            estimated: self.estimated,
            partial: self.partial,
            executions: self.executions,
            readable: self.readable,
            unknown: self.unknown.max(0),
            average_usd: (self.readable > 0).then_some(self.readable_usd / self.readable as f64),
            unpriced_models: self.unpriced_models.iter().cloned().collect(),
            missing_reasons: self.missing_reasons.iter().cloned().collect(),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct CostAggregateAcc {
    total: CostMetricAcc,
    harnesses: BTreeMap<String, CostMetricAcc>,
}

impl CostAggregateAcc {
    fn add_run(&mut self, contributions: &[crate::run_cost::CostContribution]) {
        let all: Vec<_> = contributions.iter().collect();
        self.total.add_run(&all);
        let mut names = BTreeSet::new();
        for contribution in contributions {
            if contribution.executions > 0 || contribution.usd.is_some() {
                names.insert(contribution.harness.clone());
            }
        }
        for harness in names {
            let matching: Vec<_> = contributions
                .iter()
                .filter(|contribution| contribution.harness == harness)
                .collect();
            self.harnesses
                .entry(harness)
                .or_default()
                .add_run(&matching);
        }
    }

    fn add_contribution(&mut self, contribution: &crate::run_cost::CostContribution) {
        self.total.add_contribution(contribution);
        self.harnesses
            .entry(contribution.harness.clone())
            .or_default()
            .add_contribution(contribution);
    }

    fn wire(&self) -> StatsCostAggregate {
        let mut aggregate = self.total.wire();
        aggregate.harnesses = self
            .harnesses
            .iter()
            .map(|(harness, metric)| metric.wire_harness(harness.clone()))
            .collect();
        aggregate
    }
}

#[derive(Debug, Clone, Default)]
struct CostEntityAcc {
    name: String,
    aggregate: CostAggregateAcc,
    periods: BTreeMap<String, CostAggregateAcc>,
    nodes: BTreeMap<String, CostEntityAcc>,
}

#[derive(Debug, Clone, Default)]
struct CostProjectAcc {
    name: String,
    aggregate: CostAggregateAcc,
    periods: BTreeMap<String, CostAggregateAcc>,
    pipelines: BTreeMap<String, CostEntityAcc>,
}

fn wire_periods(periods: BTreeMap<String, CostAggregateAcc>) -> Vec<StatsCostPeriod> {
    periods
        .into_iter()
        .map(|(bucket, aggregate)| StatsCostPeriod {
            bucket,
            aggregate: aggregate.wire(),
        })
        .collect()
}

fn wire_cost_entity(id: String, entity: CostEntityAcc) -> StatsCostEntity {
    let mut nodes: Vec<_> = entity
        .nodes
        .into_iter()
        .map(|(id, node)| wire_cost_entity(id, node))
        .collect();
    nodes.sort_by(|a, b| {
        b.aggregate
            .usd
            .unwrap_or(-1.0)
            .partial_cmp(&a.aggregate.usd.unwrap_or(-1.0))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    StatsCostEntity {
        id,
        name: entity.name,
        aggregate: entity.aggregate.wire(),
        by_period: wire_periods(entity.periods),
        nodes,
    }
}

/// Project the resolved price table into wire rows (#528): one entry per family
/// key — the winning tier and the `$/MTok` in force — in `BTreeMap` order. Reads
/// the SAME `resolved` map `price_for` bills from, so the Cost tab can never
/// enumerate a set the pricer would price otherwise (#373). Pure; the handler
/// injects the live table. `PriceTable::load` always seeds the embedded floor, so
/// this never yields `[]` even with no HOME state (D9).
fn resolved_price_rows(prices: &crate::price_table::PriceTable) -> Vec<ResolvedPriceRow> {
    prices
        .resolved_entries()
        .map(|(key, price, tier)| ResolvedPriceRow {
            key: key.to_string(),
            tier,
            input: price.input,
            output: price.output,
        })
        .collect()
}

/// The "by project" bucket of a cost row: the Run's `target_repo`, else the
/// daemon repo root. No "Unassigned" bucket (invariant #6, #258).
///
/// This reads the raw `run_started` payload rather than projecting a full
/// `RunState`, so the SQL fold stays cheap — it is a deliberate inline copy of
/// `effective_repo_root`, named here so it can be tested.
///
/// #470/ADR-0033: do NOT symmetrise this with the hardened write boundary.
/// `run_started` events recorded before #470 legitimately carry
/// `target_repo: null` (≈ 46 of 101 dev runs), and resolving them here is exactly
/// what buys the "no Unassigned bucket" invariant. The asymmetry is the design:
/// required where there is a caller to answer 400 to, resolved where there is
/// only a past record to interpret.
fn cost_project_root(payload: &serde_json::Value, daemon_root: &Path) -> PathBuf {
    payload
        .get("target_repo")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .unwrap_or_else(|| daemon_root.to_path_buf())
}

struct CostRunRow {
    bucket: String,
    pipeline_id: String,
    pipeline_name: String,
    project_id: String,
    project_name: String,
    node_names: BTreeMap<String, String>,
    contributions: Vec<crate::run_cost::CostContribution>,
}

fn fold_harness_cost(runs: &[CostRunRow], resolved: Vec<ResolvedPriceRow>) -> StatsCost {
    let mut total = CostAggregateAcc::default();
    let mut periods = BTreeMap::<String, CostAggregateAcc>::new();
    let mut pipelines = BTreeMap::<String, CostEntityAcc>::new();
    let mut projects = BTreeMap::<String, CostProjectAcc>::new();
    let mut active_harnesses = BTreeSet::new();

    for run in runs {
        total.add_run(&run.contributions);
        periods
            .entry(run.bucket.clone())
            .or_default()
            .add_run(&run.contributions);

        let pipeline = pipelines.entry(run.pipeline_id.clone()).or_default();
        pipeline.name = run.pipeline_name.clone();
        pipeline.aggregate.add_run(&run.contributions);
        pipeline
            .periods
            .entry(run.bucket.clone())
            .or_default()
            .add_run(&run.contributions);

        let project = projects.entry(run.project_id.clone()).or_default();
        project.name = run.project_name.clone();
        project.aggregate.add_run(&run.contributions);
        project
            .periods
            .entry(run.bucket.clone())
            .or_default()
            .add_run(&run.contributions);
        let project_pipeline = project
            .pipelines
            .entry(run.pipeline_id.clone())
            .or_default();
        project_pipeline.name = run.pipeline_name.clone();
        project_pipeline.aggregate.add_run(&run.contributions);
        project_pipeline
            .periods
            .entry(run.bucket.clone())
            .or_default()
            .add_run(&run.contributions);

        for contribution in &run.contributions {
            if contribution.executions > 0 {
                active_harnesses.insert(contribution.harness.clone());
            }
            let (id, name) = match contribution.scope {
                crate::run_cost::CostScope::Node => {
                    let id = contribution
                        .node_id
                        .clone()
                        .unwrap_or_else(|| "(unknown)".to_string());
                    let name = run
                        .node_names
                        .get(&id)
                        .cloned()
                        .unwrap_or_else(|| id.clone());
                    (id, name)
                }
                crate::run_cost::CostScope::Infrastructure => (
                    format!("{}:infrastructure", run.pipeline_id),
                    "Infrastructure".to_string(),
                ),
                crate::run_cost::CostScope::Unassigned => (
                    format!("{}:unassigned", run.pipeline_id),
                    "Unassigned".to_string(),
                ),
            };
            let node = pipeline.nodes.entry(id.clone()).or_default();
            node.name = name.clone();
            node.aggregate.add_contribution(contribution);
            node.periods
                .entry(run.bucket.clone())
                .or_default()
                .add_contribution(contribution);

            let project_node = project_pipeline.nodes.entry(id).or_default();
            project_node.name = name;
            project_node.aggregate.add_contribution(contribution);
            project_node
                .periods
                .entry(run.bucket.clone())
                .or_default()
                .add_contribution(contribution);
        }
    }

    let mut by_pipeline: Vec<_> = pipelines
        .into_iter()
        .map(|(id, pipeline)| wire_cost_entity(id, pipeline))
        .collect();
    by_pipeline.sort_by(|a, b| {
        b.aggregate
            .usd
            .unwrap_or(-1.0)
            .partial_cmp(&a.aggregate.usd.unwrap_or(-1.0))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut by_project: Vec<_> = projects
        .into_iter()
        .map(|(id, project)| {
            let mut pipelines: Vec<_> = project
                .pipelines
                .into_iter()
                .map(|(id, pipeline)| wire_cost_entity(id, pipeline))
                .collect();
            pipelines.sort_by(|a, b| {
                b.aggregate
                    .usd
                    .unwrap_or(-1.0)
                    .partial_cmp(&a.aggregate.usd.unwrap_or(-1.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.id.cmp(&b.id))
            });
            StatsProjectCostEntity {
                entity: StatsCostEntity {
                    id,
                    name: project.name,
                    aggregate: project.aggregate.wire(),
                    by_period: wire_periods(project.periods),
                    nodes: Vec::new(),
                },
                pipelines,
            }
        })
        .collect();
    by_project.sort_by(|a, b| {
        b.entity
            .aggregate
            .usd
            .unwrap_or(-1.0)
            .partial_cmp(&a.entity.aggregate.usd.unwrap_or(-1.0))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.entity.id.cmp(&b.entity.id))
    });

    StatsCost {
        harnesses: active_harnesses.into_iter().collect(),
        total: total.wire(),
        by_period: wire_periods(periods),
        by_pipeline,
        by_project,
        resolved,
    }
}

/// `GET /stats/cost` — Class B, memo + app-side fold. Heavy (fans over the
/// `~/.claude` corpus); fetched lazily by the client only when the cost tab is
/// shown.
pub(crate) async fn stats_cost(
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

    // #408: resolve the sandbox home roots once for the whole fold. HOME absent →
    // degrade to the host `~/.claude` root (never fail the aggregate).
    let (home_root, sandbox_root) =
        crate::sandbox_run::sandbox_home_roots(&state).unwrap_or_else(|_| {
            let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
            let sandbox = home.join(".pdo").join("sandbox");
            (home, sandbox)
        });

    // #427: the three price tiers, resolved ONCE for the whole fold — never inside
    // the per-Run loop. `home_root` is the HOST home even for a sandboxed Run:
    // prices are an instance concept, and the #408 seam moves the TRANSCRIPT root,
    // not this one. The table's fingerprint is the memo's third key component, so a
    // sync is visible here without a daemon restart.
    let prices = crate::price_table::PriceTable::load(&home_root);
    // `copilot`'s store is always the host journal (no staging set); `pi`'s moves per
    // Run (#708) — the staged sink while a sandboxed Run lives, the host store after
    // merge-back — so `stores` is rebuilt inside the per-Run loop below, not here.
    let stored_projects = match crate::project_store::list(&state.db).await {
        Ok(projects) => projects,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("stats cost failed: {error}") })),
            )
                .into_response();
        }
    };

    let mut cost_rows: Vec<CostRunRow> = Vec::with_capacity(rows.len());
    for (run_id, bucket, payload) in rows {
        let payload: serde_json::Value = payload
            .as_deref()
            .and_then(|p| serde_json::from_str(p).ok())
            .unwrap_or(serde_json::Value::Null);

        // Pipeline key: `pipeline_id` going forward (#377), else the (always
        // present) `pipeline_name` — so grouping survives a rename (#230).
        let pipeline_id = payload
            .get("pipeline_id")
            .and_then(|v| v.as_str())
            .or_else(|| payload.get("pipeline_name").and_then(|v| v.as_str()))
            .unwrap_or("(unknown)")
            .to_string();
        let pipeline_name = payload
            .get("pipeline_name")
            .and_then(|value| value.as_str())
            .unwrap_or(&pipeline_id)
            .to_string();

        let repo_root = cost_project_root(&payload, &state.repo_root);
        let (project_id, project_name) =
            project_identity(&payload, &state.repo_root, &stored_projects);

        // #408: read the transcripts from the sandboxed Run's staged home while it
        // is live (else `~/.claude/projects/`). Read `sandbox` straight off the
        // `run_started` payload (like `target_repo`/`pipeline_id`) — no full
        // RunState projection, so the SQL stays cheap (no fan-out regression).
        //
        // #432: this stops being a *decoder*. It used to `from_value::<SandboxMode>`,
        // which silently swallowed any token the closed enum did not know; all this
        // fold ever needed is the off-ness, and asking the profile store whether the
        // name resolves would be an N+1 inside a per-row loop of a SQL fan-out.
        let sandboxed = payload
            .get("sandbox")
            .and_then(|v| v.as_str())
            .is_some_and(|s| {
                let t = s.trim();
                !t.is_empty() && !t.eq_ignore_ascii_case(crate::event_log::SandboxMode::OFF_WIRE)
            });
        let projects_root =
            crate::sandbox_run::transcripts_root(sandboxed, &run_id, &home_root, &sandbox_root);
        // #708: pi's store follows the same sandbox-aware seam as the Claude root.
        let stores = crate::sandbox_run::HarnessStores::for_run(
            sandboxed,
            &run_id,
            &home_root,
            &sandbox_root,
        );
        let events = match crate::load_events(&state.db, &run_id).await {
            Ok(events) => events,
            Err(error) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("stats cost failed: {error}") })),
                )
                    .into_response();
            }
        };
        let breakdown = crate::run_cost::compute_run_cost_breakdown_cached(
            &events,
            &projects_root,
            &stores,
            &repo_root,
            &run_id,
            &prices,
        );
        let node_names = payload
            .get("node_defs")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .filter_map(|node| {
                let id = node.get("id")?.as_str()?.to_string();
                let name = node
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or(&id)
                    .to_string();
                Some((id, name))
            })
            .collect();
        cost_rows.push(CostRunRow {
            bucket,
            pipeline_id,
            pipeline_name,
            project_id,
            project_name,
            node_names,
            contributions: breakdown.contributions,
        });
    }

    let stats = fold_harness_cost(&cost_rows, resolved_price_rows(&prices));
    Json(stats).into_response()
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

    #[tokio::test]
    async fn init_db_installs_the_idempotent_session_cohort_join_index() {
        let db = mem_db().await;
        crate::init_db(&db).await.unwrap();
        let columns = sqlx::query_scalar::<_, String>(
            "SELECT name FROM pragma_index_info('idx_events_run_kind_id') ORDER BY seqno",
        )
        .fetch_all(&db)
        .await
        .unwrap();
        assert_eq!(columns, vec!["run_id", "kind", "id"]);
    }

    /// Insert a `run_started` + a terminal event for a run. `target_repo` is an
    /// `Option` because a Run recorded before #470 legitimately has none, and the
    /// "by project" bucket must still place it (ADR-0033).
    async fn seed_run(
        db: &sqlx::SqlitePool,
        run_id: &str,
        pipeline_name: &str,
        target_repo: Option<&str>,
        day: &str,
        terminal: &str,
    ) {
        let mut payload_json = serde_json::json!({ "pipeline_name": pipeline_name });
        if let Some(repo) = target_repo {
            payload_json["target_repo"] = serde_json::json!(repo);
        }
        let payload = payload_json.to_string();
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
        seed_run(
            db,
            "r1",
            "alpha",
            Some("/proj/A"),
            "2026-07-15",
            "run_completed",
        )
        .await;
        seed_run(
            db,
            "r2",
            "alpha",
            Some("/proj/A"),
            "2026-07-15",
            "run_failed",
        )
        .await;
        seed_run(
            db,
            "r3",
            "beta",
            Some("/proj/B"),
            "2026-07-16",
            "run_completed",
        )
        .await;
        seed_run(
            db,
            "r4",
            "beta",
            Some("/proj/B"),
            "2026-07-16",
            "run_completed",
        )
        .await;
        seed_run(
            db,
            "r5",
            "alpha",
            Some("/proj/A"),
            "2026-07-17",
            "run_skipped",
        )
        .await;
        seed_run(
            db,
            "r6",
            "beta",
            Some("/proj/B"),
            "2026-07-17",
            "run_failed",
        )
        .await;
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
        let total_sessions: i64 = ov.sessions.iter().map(|b| b.count).sum();
        assert_eq!(total_sessions, 3);
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
    fn resolved_price_rows_project_the_floor_faithfully() {
        // The projection `resolved_entries -> ResolvedPriceRow` is faithful: the
        // fourteen embedded families (since #527 floored the current generation),
        // every one `Embedded`, with the single most error-prone distinction
        // surviving (opus-4-8 5/25 ≠ opus-4-1 15/75), in BTreeMap key order. The
        // winning-tier PRECEDENCE (manual > fetched > embedded) is exercised by
        // `price_table::resolved_entries_*` and end-to-end over `/stats/cost` in
        // `tests/cost_prices.rs`.
        use crate::price_table::{PriceTable, PriceTier};
        let floor = resolved_price_rows(&PriceTable::builtin());
        assert_eq!(floor.len(), 14);
        assert!(floor.iter().all(|r| r.tier == PriceTier::Embedded));
        let by = |key: &str| floor.iter().find(|r| r.key == key).unwrap();
        assert_eq!(
            (by("claude-opus-4-8").input, by("claude-opus-4-8").output),
            (5.0, 25.0)
        );
        assert_eq!(
            (by("claude-opus-4-1").input, by("claude-opus-4-1").output),
            (15.0, 75.0)
        );
        // Rows come out in BTreeMap key order (families grouped for free, D4).
        let keys: Vec<&str> = floor.iter().map(|r| r.key.as_str()).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted);
        // The sentinel is a `price_for` short-circuit, never a table row.
        assert!(floor.iter().all(|r| r.key != "<synthetic>"));
    }

    #[test]
    fn cost_project_root_uses_the_runs_target_repo() {
        let payload = serde_json::json!({ "target_repo": "/proj/A" });
        assert_eq!(
            cost_project_root(&payload, Path::new("/daemon/root")),
            PathBuf::from("/proj/A")
        );
    }

    #[test]
    fn unnamed_project_uses_the_primary_repository_identity_and_readable_name() {
        let payload = serde_json::json!({ "target_repo": "/repos/product-api" });
        assert_eq!(
            project_identity(&payload, Path::new("/daemon/root"), &[]),
            ("/repos/product-api".to_string(), "product-api".to_string())
        );
    }

    #[test]
    fn harness_cost_fold_keeps_lower_bounds_and_unknown_columns_honest() {
        let run = CostRunRow {
            bucket: "2026-07-15".to_string(),
            pipeline_id: "p".to_string(),
            pipeline_name: "Pipeline".to_string(),
            project_id: "/repo".to_string(),
            project_name: "repo".to_string(),
            node_names: BTreeMap::from([("n".to_string(), "Node".to_string())]),
            contributions: vec![
                crate::run_cost::CostContribution {
                    harness: "claude".to_string(),
                    scope: crate::run_cost::CostScope::Node,
                    node_id: Some("n".to_string()),
                    executions: 1,
                    readable_executions: 1,
                    usd: Some(0.0),
                    form: Some(crate::event_log::CostForm::Derived),
                    reported_in_usd: false,
                    partial: true,
                    unpriced_models: vec!["claude-fable-5".to_string()],
                    unavailable_reasons: Vec::new(),
                },
                crate::run_cost::CostContribution {
                    harness: "future".to_string(),
                    scope: crate::run_cost::CostScope::Node,
                    node_id: Some("n".to_string()),
                    executions: 1,
                    readable_executions: 0,
                    usd: None,
                    form: None,
                    reported_in_usd: false,
                    partial: false,
                    unpriced_models: Vec::new(),
                    unavailable_reasons: vec!["harness has no cost source".to_string()],
                },
            ],
        };

        let stats = fold_harness_cost(&[run], Vec::new());
        assert_eq!(stats.harnesses, vec!["claude", "future"]);
        assert_eq!(stats.total.usd, Some(0.0));
        assert!(stats.total.partial);
        assert_eq!(stats.total.executions, 1);
        assert_eq!(stats.total.readable, 0);
        assert_eq!(stats.total.unknown, 1);
        assert_eq!(stats.total.average_usd, None);
        assert_eq!(stats.total.unpriced_models, vec!["claude-fable-5"]);
        let claude = &stats.total.harnesses[0];
        assert_eq!(claude.average_usd, Some(0.0));
        assert_eq!(claude.readable, 1);
        let future = &stats.total.harnesses[1];
        assert_eq!(future.usd, None);
        assert_eq!(future.unknown, 1);
        assert_eq!(future.missing_reasons, vec!["harness has no cost source"]);
    }

    #[test]
    fn cost_project_root_buckets_a_legacy_null_target_run_under_the_daemon_root() {
        // #470/ADR-0033: the write boundary is hardened, this READ is not. A
        // `run_started` from before the change carries no `target_repo`, and the
        // "by project" axis must still place it — there is no "Unassigned" bucket
        // (invariant #6, #258). Removing this fallback would make ~46 of 101 dev
        // runs vanish from the cost cockpit.
        let payload = serde_json::json!({ "pipeline_name": "alpha" });
        assert_eq!(
            cost_project_root(&payload, Path::new("/daemon/root")),
            PathBuf::from("/daemon/root")
        );
    }

    #[tokio::test]
    async fn overview_counts_a_legacy_null_target_run() {
        // Companion to the above at the SQL layer: a null-target Run is an
        // ordinary Run everywhere on the read side.
        let db = mem_db().await;
        seed_run(&db, "legacy", "alpha", None, "2026-07-15", "run_completed").await;
        let ov = compute_overview(&db, "%Y-%m-%d", FROM, TO).await.unwrap();
        assert_eq!(ov.runs.len(), 1);
        assert_eq!(ov.runs[0].count, 1);
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
