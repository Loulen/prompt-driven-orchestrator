//! `GET /stats/overview`, `/stats/cost` and `/stats/performance`: the three
//! reads of the Stats cockpit (ADR-0029). Everything is derived on read — no
//! snapshot table, no metric-freezing event (ADR-0022).

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use super::error;
use crate::application::performance::{PerformanceError, PerformanceRead};
use crate::application::read_overview::{ReadError, StatsRead};
use crate::Stats;

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
    /// « Runs terminés seulement » (#810): narrow the cohort to Runs that
    /// reached `RunCompleted`. Wire default `false` — the whole cohort, whatever
    /// each Run's final status. Shared verbatim with `/stats/performance` so the
    /// three sections describe the same Runs; Overview then shows **zero
    /// errors** rather than hiding its card (nothing is masked).
    #[serde(default)]
    pub completed_only: bool,
    /// « Uncombined » (#888, ADR-0077): read the rows as the event log wrote
    /// them, without substituting absorbed Pipelines. Wire default `false` —
    /// absorptions applied.
    #[serde(default)]
    pub uncombined: bool,
    /// « Roll up child runs » (ADR draft-cout-de-l-arbre §2) — `/stats/cost`
    /// only: recompose the cohort by **root** Run (roots started in the period,
    /// each bringing its whole tree) and file every descendant under its
    /// root's Pipeline and Project. Wire default `false` — the response is
    /// then strictly the one without the parameter. « By model » ignores it.
    #[serde(default)]
    pub roll_up_children: bool,
}

impl StatsQuery {
    fn read(self) -> StatsRead {
        StatsRead {
            from: self.from,
            to: self.to,
            bucket: self.bucket,
            completed_only: self.completed_only,
            uncombined: self.uncombined,
        }
    }
}

/// Query string: an ISO-8601 `[from, to)` window — the same cohort rule as
/// `/stats/overview` and `/stats/cost`, but no `bucket`: Performance has no
/// time-series axis (out of scope, "Afficher des graphes temporels").
#[derive(Debug, Deserialize)]
pub struct PerformanceQuery {
    pub from: String,
    pub to: String,
    /// Explicit user refresh bypasses an otherwise-current memo entry.
    #[serde(default)]
    pub refresh: bool,
    /// « Runs terminés seulement » (#810): narrow the cohort to Runs started in
    /// the window that reached `RunCompleted`. Wire default `false` — the whole
    /// cohort, whatever each Run's final status — shared verbatim with
    /// `/stats/overview` and `/stats/cost` so the three sections describe the
    /// same Runs.
    #[serde(default)]
    pub completed_only: bool,
    /// « Uncombined » (#888, ADR-0077): no absorption applied. Shared verbatim
    /// with `/stats/overview` and `/stats/cost`.
    #[serde(default)]
    pub uncombined: bool,
}

fn read_error(e: ReadError) -> Response {
    match e {
        ReadError::InvalidBucket(bucket) => {
            error(StatusCode::BAD_REQUEST, format!("invalid bucket: {bucket}"))
        }
        ReadError::Failed(message) => error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}

/// `GET /stats/overview` — Class A cheap indexed SQL.
pub(super) async fn overview(
    State(stats): State<Arc<Stats>>,
    Query(q): Query<StatsQuery>,
) -> Response {
    match stats.read_overview().execute(&q.read()).await {
        Ok(overview) => Json(overview).into_response(),
        Err(e) => read_error(e),
    }
}

/// `GET /stats/cost` — Class B, memo + app-side fold. Heavy (fans over the
/// `~/.claude` corpus); fetched lazily by the client only when the cost tab is
/// shown.
pub(super) async fn cost(State(stats): State<Arc<Stats>>, Query(q): Query<StatsQuery>) -> Response {
    let roll_up = q.roll_up_children;
    let read = q.read();
    let cost = if roll_up {
        stats.read_cost().execute_rolled_up(&read).await
    } else {
        stats.read_cost().execute(&read).await
    };
    match cost {
        Ok(cost) => Json(cost).into_response(),
        Err(e) => read_error(e),
    }
}

/// `GET /stats/performance` — Class B heavy read (fans over the harness
/// transcript corpora). The client owns the "load on demand / Refresh" half;
/// this handler never runs on a timer.
pub(super) async fn performance(
    State(stats): State<Arc<Stats>>,
    Query(q): Query<PerformanceQuery>,
) -> Response {
    let read = PerformanceRead {
        from: q.from,
        to: q.to,
        refresh: q.refresh,
        completed_only: q.completed_only,
        uncombined: q.uncombined,
    };
    match stats.read_performance().execute(&read).await {
        Ok(payload) => Json(payload).into_response(),
        Err(PerformanceError::SourceUnreadable(message)) => {
            error(StatusCode::INTERNAL_SERVER_ERROR, message)
        }
        Err(PerformanceError::Db(e)) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("stats performance failed: {e}"),
        ),
    }
}
