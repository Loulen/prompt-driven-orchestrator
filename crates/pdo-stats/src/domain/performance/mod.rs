//! The Performance section (#585): context, duration and steering by Node and
//! harness, with the « By model » axis. See `application::performance` for
//! the fold and what counts as an observation.

pub mod acc;
pub mod model_axis;
pub mod node_defs;
pub mod waits;

use serde::Serialize;
use ts_rs::TS;

use crate::domain::absorption::tallies::AbsorbedMember;
use crate::domain::distribution::SixStats;
use crate::domain::provenance::StatsProvenance;

/// One metric's coverage for one row × one harness. **Never null on the wire**:
/// an all-or-nothing `Option` would drop `expected`/`missing_reasons` for a
/// fully absent metric, leaving the frontend unable to distinguish "never ran"
/// from "no reliable bounds". Only `stats` goes `null`, when `measured == 0`.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct StatsDistribution {
    pub stats: Option<SixStats>,
    pub measured: i64,
    pub expected: i64,
    pub missing_reasons: Vec<String>,
}

/// The steered-executions rate's raw material (#792): `steered` executions with
/// at least one steering message, over `readable` executions whose count could
/// be read. The client renders « n % · steered/readable », or « — » when
/// `readable == 0` — so the numbers travel, never a pre-computed percentage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "StatsSteeredRate")]
pub struct SteeredRate {
    pub steered: u32,
    pub readable: u32,
}

/// Matches the frontend's `StatsHarnessPerformance` — `context`/`duration`/
/// `active_duration`/`wait_duration`/`steering` are never `null` (see
/// [`StatsDistribution`]).
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct StatsHarnessPerformance {
    pub harness: String,
    pub context: StatsDistribution,
    pub duration: StatsDistribution,
    /// **Durée active** (#810): the same executions as `duration`, each minus
    /// its **declared wait** — always present beside the wall-clock, never
    /// replacing it, so the client swaps the two readings with no refetch. See
    /// [`declared_waits`] for what opens and closes a wait.
    pub active_duration: StatsDistribution,
    /// **Attente déclarée** (#819): the third reading of the very same
    /// executions — the declared wait ALONE, the human brain time an execution
    /// cost. `duration = active_duration + wait_duration` execution by
    /// execution (both derived from one wall-clock, so the identity holds up to
    /// the zero clamp). Zero is data, not an absence: a node nobody ever waited
    /// on measures `0`, and a subagent measures `0` by construction.
    pub wait_duration: StatsDistribution,
    /// Steering messages per execution (#792) — additive, unit « messages ».
    pub steering: StatsDistribution,
    /// Share of executions with ≥ 1 steering message (#792).
    pub steered: SteeredRate,
}

/// Matches the frontend's `StatsPerformanceAggregate`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, TS)]
#[ts(export, rename = "StatsPerformanceAggregate")]
pub struct PerformanceAggregate {
    pub harnesses: Vec<StatsHarnessPerformance>,
}

/// One node in the Pipeline→Node / Infrastructure→role / subagent-group tree —
/// one recursive shape for all three, matching the frontend's
/// `StatsPerformanceEntity`. A Pipeline's `subagents` is always empty (they are
/// declared on Nodes); a Node's / subagent-group's / infra role's `nodes` is
/// always empty.
#[derive(Debug, Clone, Default, PartialEq, Serialize, TS)]
#[ts(export, rename = "StatsPerformanceEntity")]
pub struct PerformanceEntity {
    pub id: String,
    pub name: String,
    pub harnesses: Vec<StatsHarnessPerformance>,
    pub nodes: Vec<PerformanceEntity>,
    pub subagents: Vec<PerformanceEntity>,
    /// The node's **genre** (#810), on Node rows only — `None` (omitted on the
    /// wire) on a Pipeline, a subagent group, an Infrastructure role or a level
    /// of the « By model » tree, which have no kind and are never filtered by
    /// it. Read off the execution's frozen `NodeStarted` (ADR-0007), with the
    /// Run's pipeline snapshot as the fallback for a payload that predates the
    /// freeze. Both flags travel: a node can be interactive AND orchestrator,
    /// and then matches either chip.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub interactive: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub orchestrator: Option<bool>,
    /// The Node's observations split into model × effort couples (ADR-0065) —
    /// Node leaves only, so the drill-down ends there; omitted (never an empty
    /// array) on the other levels, and on the `by_model` Node rows (the path is
    /// already the drill).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(optional = nullable)]
    pub models: Vec<PerformanceModelEffortPair>,
    /// Pipeline, Node and « By model » model rows (#890, #892): the Runs
    /// counted, the most recent one's start, and — on an absorbent — its
    /// absorbed members. Omitted on the other levels.
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

/// One model × effort couple of a Node leaf (ADR-0065): the performance
/// analogue of `stats::StatsModelEffortPair` — the same aggregate shape plus
/// the pair identity and where each half was read from. `effort = None` is the
/// "not set" bucket — never merged with a real effort.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct PerformanceModelEffortPair {
    #[serde(flatten)]
    pub aggregate: PerformanceAggregate,
    pub model: String,
    pub model_provenance: StatsProvenance,
    pub effort: Option<String>,
    pub effort_provenance: Option<StatsProvenance>,
    /// Same #906 fields as `stats::StatsModelEffortPair`: the couple's key,
    /// its Runs, its local members and the raw couples a global absorption
    /// counts here.
    pub key: String,
    pub runs: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_run: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(optional = nullable)]
    pub absorbed: Vec<AbsorbedMember>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(optional = nullable)]
    pub global_absorbed: Vec<AbsorbedMember>,
}

/// One effort level under a model, in the Performance « By model » tree: the
/// entity's `id` is the effort string ("" for not set) and its `name` the
/// effort or "not set".
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct PerformanceEffortEntity {
    #[serde(flatten)]
    pub entity: PerformanceEntity,
    pub effort: Option<String>,
    pub provenance: Option<StatsProvenance>,
    pub pipelines: Vec<PerformanceEntity>,
}

/// One model (verbatim id) of the Performance « By model » axis, with its
/// effort tree: Model → Effort → Pipeline → Node. Both harnesses run on the
/// same id are one row, the harness staying a column (ADR-0065 §2).
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct StatsModelPerformanceEntity {
    #[serde(flatten)]
    pub entity: PerformanceEntity,
    pub provenance: StatsProvenance,
    pub efforts: Vec<PerformanceEffortEntity>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct StatsPerformance {
    /// Every harness with at least one successful observation in the cohort.
    pub harnesses: Vec<String>,
    /// Every successful main-session Node observation pooled across the WHOLE
    /// cohort — pool the raw observations, never average the averages.
    /// Deliberately excludes Infrastructure roles: a whole Run's or resolver's
    /// wall-clock is not a Node execution. Scoped assumption, unspecified by
    /// the issue.
    pub total: PerformanceAggregate,
    pub by_pipeline: Vec<PerformanceEntity>,
    pub infrastructure: Vec<PerformanceEntity>,
    /// Both Infrastructure roles pooled at the **raw-observation** level, never
    /// a mean of the two roles' means — so the client's master/detail boxplot
    /// never has to fabricate one by averaging averages. Never pools a role's
    /// `subagents` (same rule as [`Self::total`]).
    pub infrastructure_total: PerformanceAggregate,
    /// The « By model » axis (ADR-0065): the SAME successful main-session Node
    /// observations as `total`, bucketed by the model of the session **file**
    /// each observation came from (a subagent file keeps its own bucket). Node
    /// executions only — Infrastructure roles carry no model identity and stay
    /// on `by_pipeline` (module doc, « The « By model » axis »).
    pub by_model: Vec<StatsModelPerformanceEntity>,
    /// **Couverture de l'attente** (#819): how many of [`Self::executions`]
    /// declared at least one wait. Counted over the Node executions this
    /// response folded — Infrastructure roles and subagents excluded, because
    /// neither declares a wait of its own. The client reads the pair as « n
    /// executions of N declared at least one wait »: for the others, Active =
    /// Total and Waiting = 0, which is the whole history before declared waits
    /// existed. Never inferred from a transcript or a silence.
    pub waited_executions: i64,
    /// Node executions of the cohort — the denominator of
    /// [`Self::waited_executions`].
    pub executions: i64,
}
