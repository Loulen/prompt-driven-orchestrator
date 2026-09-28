//! The Overview section (#377, ADR-0029): Runs, errors and sessions per
//! period, the fires by Pipeline, and the sessions split by harness and by
//! Pipeline → Node.

pub mod fires;
pub mod sessions;

use std::collections::BTreeMap;

use serde::Serialize;
use ts_rs::TS;

use crate::domain::absorption::tallies::AbsorbedMember;

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export, rename = "StatsBucketCount")]
pub struct BucketCount {
    pub bucket: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export, rename = "StatsPipelineFireCount")]
pub struct PipelineFireCount {
    /// The trigger's `pipeline_id`, or `"(deleted trigger)"` for an orphan fire
    /// (the trigger row was deleted; there is no cascade, so the fire survives
    /// and must be surfaced, never dropped — hence the `LEFT JOIN`).
    pub pipeline_id: String,
    /// What the chart shows (#891 — never the key): the name of the Pipeline's
    /// Sessions row, else the stored name of an absorbent, else the key itself.
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export, rename = "StatsTriggersCreatedRuns")]
pub struct TriggersCreatedRuns {
    /// Fires whose `outcome = 'fired'` (⟺ a run was created) in the window.
    pub fired: i64,
    /// Distinct triggers that fired at least once in the window.
    pub distinct_triggers: i64,
    /// Triggers currently `enabled` (a point-in-time count, not windowed).
    pub enabled_triggers: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct StatsOverview {
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
pub struct HarnessCount {
    pub total: u64,
    pub by_harness: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct StatsSessionHarness {
    pub harness: String,
    pub executions: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct StatsSessionPeriod {
    pub bucket: String,
    pub harnesses: Vec<StatsSessionHarness>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct StatsSessionEntity {
    pub id: String,
    pub name: String,
    pub executions: u64,
    pub harnesses: Vec<StatsSessionHarness>,
    pub by_period: Vec<StatsSessionPeriod>,
    pub nodes: Vec<StatsSessionEntity>,
    /// Pipeline and Node rows (#890, #892): the Runs counted, the most recent
    /// one's start, and — on an absorbent — its absorbed members.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub runs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_run: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(optional = nullable)]
    pub absorbed: Vec<AbsorbedMember>,
}

/// The series of one window, as the log and the Triggers counted them.
pub struct OverviewSeries {
    pub runs: Vec<BucketCount>,
    pub errors: Vec<BucketCount>,
    pub fires_by_pipeline: Vec<PipelineFireCount>,
    pub triggers_created_runs: TriggersCreatedRuns,
    pub sessions: sessions::SessionStats,
}

/// Assemble the Overview: the ordered x-axis is the sorted union of the
/// periods of the Runs, errors and `node_started` counts (`node_starts`); the
/// Sessions series and trees come from the cohort's fold. A fired Pipeline that
/// ran in the period reads under its row's name.
pub fn assemble(series: OverviewSeries, node_starts: &[BucketCount]) -> StatsOverview {
    let mut labels: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for series in [&series.runs, &series.errors, node_starts] {
        for row in series.iter() {
            labels.insert(row.bucket.clone());
        }
    }
    let mut overview = StatsOverview {
        buckets: labels.into_iter().collect(),
        runs: series.runs,
        errors: series.errors,
        sessions: series.sessions.sessions,
        fires_by_pipeline: series.fires_by_pipeline,
        triggers_created_runs: series.triggers_created_runs,
        session_harnesses: series.sessions.harnesses,
        sessions_by_period: series.sessions.periods,
        sessions_by_pipeline: series.sessions.pipelines,
    };
    for fire in &mut overview.fires_by_pipeline {
        if let Some(row) = overview
            .sessions_by_pipeline
            .iter()
            .find(|row| row.id == fire.pipeline_id)
        {
            fire.name = row.name.clone();
        }
    }
    overview
}
