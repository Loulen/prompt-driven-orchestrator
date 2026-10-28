//! `GET /runs/costs?ids=a,b`: the cost of the Runs a list shows, derived on
//! read (ADR-0052). Apart from `GET /runs` so the global list stays cheap: the
//! list asks it for the rows it draws, once a Run has ended or now and then
//! while it runs.

use std::collections::HashSet;

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use pdo_kernel::Actor;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::domain::state::CostStat;
use crate::Runs;

/// The most Runs one read prices: each replays a log and reads its sessions.
pub const MAX_COSTED_RUNS: usize = 100;

/// One Run of `GET /runs/costs`.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct RunCostEntry {
    pub run_id: String,
    /// `None` when no source tells it: the row shows nothing, never `$0`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cost: Option<CostStat>,
}

/// `GET /runs/costs`: the Runs asked for that exist and the caller may read.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct RunCostsResponse {
    pub runs: Vec<RunCostEntry>,
}

#[derive(Deserialize)]
pub struct RunCostsQuery {
    /// The Run ids, comma-separated.
    #[serde(default)]
    pub ids: String,
}

impl Runs {
    /// `GET /runs/costs`: the cost of each Run of `query` that exists and
    /// `actor` may read (CORE-5), in the order asked. More than
    /// [`MAX_COSTED_RUNS`] ids is a 400.
    pub async fn run_costs(&self, actor: &Actor, query: RunCostsQuery) -> Response {
        let mut seen = HashSet::new();
        let ids: Vec<&str> = query
            .ids
            .split(',')
            .map(str::trim)
            .filter(|id| !id.is_empty() && seen.insert(*id))
            .collect();
        if ids.len() > MAX_COSTED_RUNS {
            return (
                StatusCode::BAD_REQUEST,
                format!("at most {MAX_COSTED_RUNS} runs per read"),
            )
                .into_response();
        }
        let mut summaries = match self.store().summaries().await {
            Ok(summaries) => summaries,
            Err(e) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
            }
        };
        summaries.retain(|summary| ids.contains(&summary.run_id.as_str()));
        self.readable(actor).retain(&mut summaries).await;
        let known: HashSet<&str> = summaries.iter().map(|s| s.run_id.as_str()).collect();
        let mut runs = Vec::with_capacity(known.len());
        for id in ids.iter().filter(|id| known.contains(*id)) {
            let cost: Option<CostStat> = self.cost_of(id).await;
            runs.push(RunCostEntry {
                run_id: (*id).to_string(),
                cost,
            });
        }
        Json(RunCostsResponse { runs }).into_response()
    }
}
