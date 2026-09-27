//! The accumulators of the Performance fold: one metric's raw observations,
//! a harness's five metrics, a Node, a Pipeline.

use std::collections::{BTreeMap, BTreeSet};

use super::model_axis::{wire_model_pairs, ModelEffortKey, ModelPairAcc};
use super::waits::active_millis;
use super::{PerformanceEntity, StatsDistribution, StatsHarnessPerformance, SteeredRate};
use crate::domain::absorption::resolver::AbsorptionResolver;
use crate::domain::absorption::tallies::PipelineTallies;
use crate::domain::distribution::r7_distribution;

#[derive(Debug, Default)]
pub struct MetricAcc {
    pub values: Vec<f64>,
    pub expected: i64,
    pub absence_reasons: BTreeSet<String>,
}

impl MetricAcc {
    pub fn observe(&mut self, value: Option<f64>, absence_reason: Option<&str>) {
        self.expected += 1;
        match value {
            Some(v) => self.values.push(v),
            None => {
                if let Some(reason) = absence_reason {
                    self.absence_reasons.insert(reason.to_string());
                }
            }
        }
    }

    /// Pool another accumulator's RAW observations into this one — never a mean
    /// of means (the whole module pools at the raw level).
    pub fn fold(&mut self, other: &MetricAcc) {
        self.values.extend_from_slice(&other.values);
        self.expected += other.expected;
        self.absence_reasons
            .extend(other.absence_reasons.iter().cloned());
    }

    /// Name why this metric is structurally absent for the row, without
    /// counting an expected observation: a subagent's Steering (nobody types in
    /// one) is « — <reason> », not a `0 of n` coverage gap.
    pub fn declare_absent(&mut self, reason: &str) {
        self.absence_reasons.insert(reason.to_string());
    }

    /// The steered-executions rate this accumulator's raw values carry (#792):
    /// every readable count is one `readable`, every count ≥ 1 one `steered`.
    pub fn steered(&self) -> SteeredRate {
        SteeredRate {
            steered: self.values.iter().filter(|v| **v >= 1.0).count() as u32,
            readable: self.values.len() as u32,
        }
    }

    pub fn finish(self) -> StatsDistribution {
        StatsDistribution {
            stats: r7_distribution(&self.values),
            measured: self.values.len() as i64,
            expected: self.expected,
            missing_reasons: self.absence_reasons.into_iter().collect(),
        }
    }
}

/// One metric's reading for one observation: the value, or the reason it is
/// absent. `Steering` for a subagent file is [`Reading::NotApplicable`] — a
/// declared absence that counts no expected observation.
#[derive(Debug, Clone, Copy)]
pub enum Reading<'a> {
    Observed(Option<f64>, Option<&'a str>),
    NotApplicable(&'a str),
}

impl MetricAcc {
    pub fn record(&mut self, reading: Reading<'_>) {
        match reading {
            Reading::Observed(value, reason) => self.observe(value, reason),
            Reading::NotApplicable(reason) => self.declare_absent(reason),
        }
    }
}

/// Why a subagent file carries no Steering (#792): no human types in one.
pub const SUBAGENT_STEERING_REASON: &str = "subagents are never steered";
/// Why an Infrastructure role carries no Steering (#792): the daemon's own
/// review batches reach the manager, no human pilots it.
pub const INFRA_STEERING_REASON: &str = "runtime messages only";

#[derive(Debug, Default)]
pub struct HarnessAcc {
    pub context: MetricAcc,
    pub duration: MetricAcc,
    /// #810 — the same executions as `duration`, declared wait subtracted.
    pub active_duration: MetricAcc,
    /// #819 — the same executions as `duration`, the declared wait alone.
    pub wait_duration: MetricAcc,
    pub steering: MetricAcc,
}

impl HarnessAcc {
    pub fn finish(self, harness: String) -> StatsHarnessPerformance {
        StatsHarnessPerformance {
            harness,
            context: self.context.finish(),
            duration: self.duration.finish(),
            active_duration: self.active_duration.finish(),
            wait_duration: self.wait_duration.finish(),
            steered: self.steering.steered(),
            steering: self.steering.finish(),
        }
    }
}

/// One execution's three duration readings, recorded together everywhere the
/// fold observes a duration: the wall-clock, minus its declared wait, and the
/// declared wait alone. The three share ONE absence reason, so the coverage
/// (`n=`) never disagrees between modes — switching mode must not change how
/// many executions the reader is looking at.
#[derive(Debug, Clone, Copy)]
pub struct DurationReading<'a> {
    pub total: Option<f64>,
    pub active: Option<f64>,
    pub waiting: Option<f64>,
    pub reason: Option<&'a str>,
}

impl<'a> DurationReading<'a> {
    /// The three readings of one execution: `waited` milliseconds of declared
    /// wait taken off `total`. The wait is clamped into `[0, total]` so the
    /// identity `Total = Active + Waiting` holds exactly, even when a clock
    /// skew reports more wait than wall-clock.
    pub fn new(total: Option<f64>, waited: f64, reason: Option<&'a str>) -> Self {
        let waiting = total.map(|value| waited.clamp(0.0, value));
        Self {
            total,
            active: active_millis(total, waited),
            waiting,
            reason,
        }
    }

    /// An execution nobody can declare a wait on (a subagent, a Merge
    /// resolver): Waiting is `0`, and Active IS the wall-clock.
    pub fn unwaited(total: Option<f64>, reason: Option<&'a str>) -> Self {
        Self::new(total, 0.0, reason)
    }
}

impl HarnessAcc {
    /// Observe one execution's three duration readings at once — the only way
    /// they are ever recorded, so no accumulator can drift out of coverage with
    /// the other two.
    pub fn observe_durations(&mut self, reading: DurationReading<'_>) {
        self.duration.observe(reading.total, reading.reason);
        self.active_duration.observe(reading.active, reading.reason);
        self.wait_duration.observe(reading.waiting, reading.reason);
    }
}

/// A Node's **genre** (#810): the two frozen flags read together. Not an enum —
/// a node can be both, and then matches either chip; « Standard » is the
/// absence of both, never a third stored value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NodeKind {
    pub interactive: bool,
    pub orchestrator: bool,
}

pub fn finish_by_harness(acc: BTreeMap<String, HarnessAcc>) -> Vec<StatsHarnessPerformance> {
    acc.into_iter().map(|(h, a)| a.finish(h)).collect()
}

#[derive(Debug, Default)]
pub struct SubagentAcc {
    pub by_harness: BTreeMap<String, HarnessAcc>,
}

#[derive(Debug, Default)]
pub struct NodeAcc {
    pub name: String,
    /// #810 — the genre of the LAST observed execution of this Node in the
    /// cohort, same "latest wins" rule as `name`: a node re-flagged mid-period
    /// reads as what it is now, and its older executions stay on the row.
    pub kind: NodeKind,
    pub by_harness: BTreeMap<String, HarnessAcc>,
    pub subagents: BTreeMap<String, SubagentAcc>,
    /// The Node's model × effort couples (ADR-0065): the same observations as
    /// `by_harness`, bucketed by the model of the session **file** each came
    /// from — the drill "Node → modèle × effort" of `by_pipeline` (#737).
    pub pairs: BTreeMap<ModelEffortKey, ModelPairAcc>,
    /// Which raw Node ids' executions landed here (#892).
    pub tallies: PipelineTallies,
}

pub fn finish_subagents(subagents: BTreeMap<String, SubagentAcc>) -> Vec<PerformanceEntity> {
    subagents
        .into_iter()
        .map(|(group, acc)| PerformanceEntity {
            id: group.clone(),
            name: group,
            harnesses: finish_by_harness(acc.by_harness),
            nodes: Vec::new(),
            subagents: Vec::new(),
            // A subagent group has no kind of its own: it follows its Node
            // (#810), which the client already renders under the parent row.
            interactive: None,
            orchestrator: None,
            models: Vec::new(),
            ..Default::default()
        })
        .collect()
}

pub fn finish_node(
    pipeline: &str,
    id: String,
    acc: NodeAcc,
    resolver: &AbsorptionResolver,
) -> PerformanceEntity {
    PerformanceEntity {
        runs: Some(acc.tallies.runs()),
        last_run: acc.tallies.last_run(),
        absorbed: resolver.absorbed_nodes(pipeline, &id, &acc.tallies),
        id,
        name: acc.name,
        harnesses: finish_by_harness(acc.by_harness),
        nodes: Vec::new(),
        subagents: finish_subagents(acc.subagents),
        interactive: Some(acc.kind.interactive),
        orchestrator: Some(acc.kind.orchestrator),
        models: wire_model_pairs(acc.pairs, resolver),
    }
}

/// Like [`finish_node`], but `id`/`name` are the role's fixed identity: an
/// infra `NodeAcc` never sets `acc.name`.
pub fn finish_infra_role(id: &str, name: &str, acc: NodeAcc) -> PerformanceEntity {
    PerformanceEntity {
        id: id.to_string(),
        name: name.to_string(),
        harnesses: finish_by_harness(acc.by_harness),
        nodes: Vec::new(),
        subagents: finish_subagents(acc.subagents),
        // An Infrastructure role is not a Node: it has no kind, and the kind
        // filter leaves it intact (#810).
        interactive: None,
        orchestrator: None,
        // Infrastructure carries no model identity (module doc): an empty
        // `models` is omitted on the wire, never an empty array.
        models: Vec::new(),
        ..Default::default()
    }
}

#[derive(Debug, Default)]
pub struct PipelineAcc {
    pub name: String,
    pub by_harness: BTreeMap<String, HarnessAcc>,
    pub nodes: BTreeMap<String, NodeAcc>,
    pub tallies: PipelineTallies,
}

pub fn finish_pipeline(
    id: String,
    acc: PipelineAcc,
    resolver: &AbsorptionResolver,
) -> PerformanceEntity {
    let absorbed = resolver.absorbed(&id, &acc.tallies);
    let nodes = acc
        .nodes
        .into_iter()
        .map(|(node_id, node)| finish_node(&id, node_id, node, resolver))
        .collect();
    PerformanceEntity {
        runs: Some(acc.tallies.runs()),
        last_run: acc.tallies.last_run(),
        absorbed,
        id,
        name: acc.name,
        harnesses: finish_by_harness(acc.by_harness),
        nodes,
        subagents: Vec::new(),
        // A Pipeline row is not a Node: no kind (#810). Under a partial kind
        // filter the client shows « filtered » here rather than recomputing a
        // total the six stats cannot rebuild.
        interactive: None,
        orchestrator: None,
        models: Vec::new(),
    }
}

// --- « By model » axis (#737, ADR-0065) ---------------------------------------
