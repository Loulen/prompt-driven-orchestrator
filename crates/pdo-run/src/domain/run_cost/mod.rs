//! The cost of a Run, as its read side folds it (ADR-0052, ADR-0065): each
//! execution's contribution, ventilated by harness and by model × effort, and
//! the identity that tells two executions apart. The files it is read from are
//! the adapters' (`adapters/run_cost/`); this is the shape and the arithmetic.

pub mod loop_cost;
pub mod tree;
pub mod usage;

use crate::domain::state::{CostForm, CostStat};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostScope {
    Node,
    Infrastructure,
    Unassigned,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CostContribution {
    pub harness: String,
    pub scope: CostScope,
    pub node_id: Option<String>,
    pub executions: i64,
    pub readable_executions: i64,
    pub usd: Option<f64>,
    pub form: Option<CostForm>,
    /// A reported cost already in dollars (constant 1.0, `pi`) — see
    /// [`HarnessCost::reported_in_usd`]. `false` for derived and converted slices.
    pub reported_in_usd: bool,
    pub partial: bool,
    pub unpriced_models: Vec<String>,
    pub unavailable_reasons: Vec<String>,
    /// The execution's cost **ventilated by model × effort** (ADR-0065) — the
    /// raw material of the « By model » axis and the Node leaves' pairs. One
    /// slice per model the execution provably ran on; empty for a harness with
    /// no cost source (absent from the axis, never a "default of X" bucket)
    /// and for a mute source with no requested model.
    pub model_slices: Vec<ModelEffortSlice>,
    /// Index, in the Run's event slice, of the `NodeStarted` that opened this
    /// execution. `None` for the infrastructure and unassigned buckets. What
    /// the **Coût de boucle** windows on: an execution counts toward a loop
    /// only when it started after the loop's last entry ([`loop_cost`]).
    pub started_event: Option<usize>,
}

/// One model bucket of one execution's cost (ADR-0065). An execution with two
/// models produces two slices and counts — and costs — in both buckets, so a
/// bucket's average per execution stays coherent with its cost (ADR-0065 §3).
#[derive(Debug, Clone, PartialEq)]
pub struct ModelEffortSlice {
    /// The model id **verbatim** (ADR-0065 §2): no aliasing, no de-dating, no
    /// fallback to the price table's undated family.
    pub model: String,
    /// `true` when the harness's source reported this id (observed); `false`
    /// when it is the startup event's requested id, read in fallback because
    /// the source was mute.
    pub model_observed: bool,
    /// The execution's effort, or `None` when neither source named one — the
    /// "not set" bucket, never merged with a real effort.
    pub effort: Option<String>,
    /// `Some(false)` = requested, `Some(true)` = observed, `None` when `effort`
    /// is `None` (not set has no provenance to show).
    pub effort_observed: Option<bool>,
    /// The provider the source named for this model (`pi` via openrouter), the
    /// tooltip's « via … » — never part of the identity (ADR-0065 §2). `None`
    /// for a source that does not name one (`claude`, `copilot`).
    pub provider: Option<String>,
    pub usd: Option<f64>,
    /// Derived (transcript × price table) — the wire's `~` flag. A reported
    /// slice (`copilot`, `pi`) and an unreadable one are not estimates of this
    /// fold.
    pub estimated: bool,
    pub partial: bool,
    pub executions: i64,
    pub unpriced_models: Vec<String>,
    pub missing_reasons: Vec<String>,
    /// The slice's tokens, when the fold counted them (`claude`'s
    /// transcripts); `None` for a reported cost, whose source gives dollars.
    pub tokens: Option<usage::TokenTotals>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunCostBreakdown {
    pub contributions: Vec<CostContribution>,
    pub cost: Option<CostStat>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FrozenExecutionIdentity {
    Session {
        harness: String,
        session_id: String,
    },
    Legacy {
        node_id: String,
        iter: i64,
        event_id: Option<i64>,
        order: usize,
    },
}

pub fn frozen_execution_identity(
    harness: &str,
    session_id: Option<&str>,
    node_id: Option<&str>,
    iter: Option<i64>,
    event_id: Option<i64>,
    order: usize,
) -> FrozenExecutionIdentity {
    match session_id.filter(|session_id| !session_id.is_empty()) {
        Some(session_id) => FrozenExecutionIdentity::Session {
            harness: harness.to_string(),
            session_id: session_id.to_string(),
        },
        None => FrozenExecutionIdentity::Legacy {
            node_id: node_id.unwrap_or("(unknown)").to_string(),
            iter: iter.unwrap_or(1),
            event_id,
            order,
        },
    }
}
