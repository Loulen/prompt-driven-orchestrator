//! The accumulators of the hierarchies: a Pipeline or Node row, a model ×
//! effort pair of a Node leaf, and the « By model » axis.

use std::collections::{BTreeMap, BTreeSet};

use pdo_run::domain::run_cost::ModelEffortSlice;

use super::metric::{CostAggregateAcc, CostMetricAcc, HarnessMetricAcc};
use super::roll_up::RollUpAcc;
use super::StatsModelEffortPair;
use crate::domain::absorption::couple_key;
use crate::domain::absorption::identity::CoupleIdentity;
use crate::domain::absorption::resolver::AbsorptionResolver;
use crate::domain::absorption::tallies::PipelineTallies;
use crate::domain::provenance::provenance;

#[derive(Debug, Clone, Default)]
pub struct CostEntityAcc {
    pub name: String,
    pub aggregate: CostAggregateAcc,
    pub periods: BTreeMap<String, CostAggregateAcc>,
    pub nodes: BTreeMap<String, CostEntityAcc>,
    /// The Node leaf's model × effort pairs (ADR-0065) — only node-level accs
    /// ever receive slices, so pipeline-level wiring emits an empty vec.
    pub pairs: BTreeMap<(String, Option<String>), ModelPairAcc>,
    /// Which raw keys' Runs landed here: Pipeline keys on a Pipeline acc
    /// (#890), Node ids on a Node acc (#892).
    pub tallies: PipelineTallies,
    /// « Roll up child runs »: filled on a root Pipeline row only.
    pub roll_up: Option<Box<RollUpAcc>>,
}

/// One model × effort bucket, accumulated over the executions that landed in
/// it. The observed/requested counters decide the wire's provenance, including
/// the « mixed » case (some executions read from the source, some fell back to
/// the startup event).
#[derive(Debug, Clone, Default)]
pub struct ModelPairAcc {
    pub metric: CostMetricAcc,
    pub harnesses: BTreeMap<String, HarnessMetricAcc>,
    pub model_observed: i64,
    pub model_requested: i64,
    pub effort_observed: i64,
    pub effort_requested: i64,
    /// #906: the Runs landed here, by globally resolved couple key (the local
    /// members), and by raw couple key for those a global absorption moved.
    pub tallies: PipelineTallies,
    pub global_tallies: PipelineTallies,
    /// The raw `(Pipeline, Node)` scopes whose couples landed here.
    pub scopes: BTreeSet<String>,
}

impl ModelPairAcc {
    /// Record which Run, under which couple identity, landed here (#906).
    pub fn record(&mut self, couple: &CoupleIdentity, run_id: &str, started_at: &str) {
        self.tallies
            .record_run_as(&couple.global_key, run_id, started_at);
        if couple.global {
            self.global_tallies
                .record_run_as(&couple.raw_key, run_id, started_at);
        }
        if !couple.scope.is_empty() {
            self.scopes.insert(couple.scope.clone());
        }
    }

    pub fn add_slice(&mut self, harness: &str, slice: &ModelEffortSlice) {
        self.metric.add_slice(slice);
        self.harnesses
            .entry(harness.to_string())
            .or_default()
            .add_slice(slice);
        if slice.model_observed {
            self.model_observed += 1;
        } else {
            self.model_requested += 1;
        }
        match slice.effort_observed {
            Some(true) => self.effort_observed += 1,
            Some(false) => self.effort_requested += 1,
            None => {}
        }
    }

    pub fn wire(
        self,
        model: String,
        effort: Option<String>,
        resolver: &AbsorptionResolver,
    ) -> StatsModelEffortPair {
        let mut aggregate = self.metric.wire();
        aggregate.harnesses = self
            .harnesses
            .into_iter()
            .map(|(harness, acc)| acc.wire(harness))
            .collect();
        let key = couple_key(&model, effort.as_deref());
        StatsModelEffortPair {
            absorbed: resolver.absorbed_couples(&self.scopes, &key, &self.tallies),
            global_absorbed: self.global_tallies.global_members(),
            runs: self.tallies.runs(),
            last_run: self.tallies.last_run(),
            key,
            aggregate,
            model_provenance: provenance(self.model_observed, self.model_requested),
            effort_provenance: effort
                .as_ref()
                .map(|_| provenance(self.effort_observed, self.effort_requested)),
            model,
            effort,
        }
    }
}

/// Sort key shared by every cost row: dollars first (a missing reading last),
/// then id — the same order the pipeline/node levels already use.
pub fn cost_then_id(
    a_usd: Option<f64>,
    a_id: &str,
    b_usd: Option<f64>,
    b_id: &str,
) -> std::cmp::Ordering {
    b_usd
        .unwrap_or(-1.0)
        .partial_cmp(&a_usd.unwrap_or(-1.0))
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| a_id.cmp(b_id))
}

pub fn wire_pairs(
    pairs: BTreeMap<(String, Option<String>), ModelPairAcc>,
    resolver: &AbsorptionResolver,
) -> Vec<StatsModelEffortPair> {
    let mut rows: Vec<StatsModelEffortPair> = pairs
        .into_iter()
        .map(|((model, effort), acc)| acc.wire(model, effort, resolver))
        .collect();
    rows.sort_by(|a, b| {
        cost_then_id(a.aggregate.usd, &a.model, b.aggregate.usd, &b.model)
            .then_with(|| a.effort.cmp(&b.effort))
    });
    rows
}

/// One pipeline level of the « By model » tree. `entity.nodes` stays empty —
/// the node level rides beside it so `wire_cost_entity` can't recurse into it.
#[derive(Debug, Clone, Default)]
pub struct ModelAxisPipelineAcc {
    pub entity: CostEntityAcc,
    pub nodes: BTreeMap<String, CostEntityAcc>,
}

/// One effort level under a model. `effort: None` is the "not set" bucket.
#[derive(Debug, Clone, Default)]
pub struct ModelEffortAcc {
    pub aggregate: CostAggregateAcc,
    pub periods: BTreeMap<String, CostAggregateAcc>,
    pub pipelines: BTreeMap<String, ModelAxisPipelineAcc>,
    pub model_observed: i64,
    pub model_requested: i64,
    pub effort_observed: i64,
    pub effort_requested: i64,
    /// Which raw efforts' Runs landed here (#906), by effort key.
    pub tallies: PipelineTallies,
}

/// One model (verbatim id) of the « By model » axis.
#[derive(Debug, Clone, Default)]
pub struct ModelAcc {
    pub aggregate: CostAggregateAcc,
    pub periods: BTreeMap<String, CostAggregateAcc>,
    pub efforts: BTreeMap<Option<String>, ModelEffortAcc>,
    pub model_observed: i64,
    pub model_requested: i64,
    /// Which raw model ids landed here, and where each was read from (#892).
    pub tallies: PipelineTallies,
}
