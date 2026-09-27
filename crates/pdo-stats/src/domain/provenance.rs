//! Where a model/effort value was read from (ADR-0065 §1), shared by Cost and
//! Performance.

use serde::Serialize;
use ts_rs::TS;

/// Where a model/effort value was read from (ADR-0065 §1): the harness's source
/// (observed — the norm, never marked) or the startup event (requested — marked
/// « ? »), or both across the bucket's executions (mixed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum StatsProvenance {
    Observed,
    Requested,
    Mixed,
}

/// Observed and requested counts decide the label; both present in one bucket is
/// the honest « mixed ». Shared with the Performance « By model » axis (#737).
pub fn provenance(observed: i64, requested: i64) -> StatsProvenance {
    match (observed > 0, requested > 0) {
        (true, false) => StatsProvenance::Observed,
        (false, true) => StatsProvenance::Requested,
        (_, _) => StatsProvenance::Mixed,
    }
}
