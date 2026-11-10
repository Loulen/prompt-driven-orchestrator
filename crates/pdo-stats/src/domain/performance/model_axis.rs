//! The « By model » axis (#737, ADR-0065): the same observations bucketed by
//! the model of the session file each came from.

use std::collections::BTreeMap;

use super::acc::{finish_by_harness, DurationReading, HarnessAcc, NodeKind, Reading};
use super::{
    PerformanceAggregate, PerformanceEffortEntity, PerformanceEntity, PerformanceModelEffortPair,
    StatsModelPerformanceEntity,
};
use crate::domain::absorption::identity::{NodeIdentity, RunIdentity};
use crate::domain::absorption::resolver::AbsorptionResolver;
use crate::domain::absorption::tallies::PipelineTallies;
use crate::domain::absorption::{couple_key, effort_key};
use crate::domain::provenance::provenance;

/// A model × effort bucket key: the **verbatim** model id (ADR-0065 §2, no
/// normalisation, no alias table) and the effort it ran with — `None` is the
/// "not set" bucket, never merged with a real effort.
pub type ModelEffortKey = (String, Option<String>);

/// The model × effort a session file's observation is attributed to
/// (ADR-0065 §1): observed from the harness's source when it speaks, else the
/// startup event's requested identity. Provenance is decided per observation —
/// a bucket whose observations mix both says « mixed ».
pub struct ModelIdentity {
    pub model: String,
    pub effort: Option<String>,
    pub model_observed: bool,
    pub effort_observed: Option<bool>,
}

/// One model × effort bucket, accumulated over the session-file observations
/// that landed in it. The observed/requested counters decide the wire's
/// provenance, including the « mixed » case.
#[derive(Debug, Default)]
pub struct ModelPairAcc {
    pub by_harness: BTreeMap<String, HarnessAcc>,
    pub model_observed: i64,
    pub model_requested: i64,
    pub effort_observed: i64,
    pub effort_requested: i64,
    /// #906, on a Node's own couples only: the Runs landed here by globally
    /// resolved couple key, by raw couple key for those a global absorption
    /// moved, and the raw `(Pipeline, Node)` scopes they came from.
    pub tallies: PipelineTallies,
    pub global_tallies: PipelineTallies,
    pub scopes: std::collections::BTreeSet<String>,
}

impl ModelPairAcc {
    /// Observe one session file's context peak + durations + steering into this
    /// bucket.
    pub fn observe(
        &mut self,
        harness: &str,
        context: (Option<f64>, Option<&str>),
        durations: DurationReading<'_>,
        steering: Reading<'_>,
        identity: &ModelIdentity,
    ) {
        let acc = self.by_harness.entry(harness.to_string()).or_default();
        acc.context.observe(context.0, context.1);
        acc.observe_durations(durations);
        acc.steering.record(steering);
        if identity.model_observed {
            self.model_observed += 1;
        } else {
            self.model_requested += 1;
        }
        match identity.effort_observed {
            Some(true) => self.effort_observed += 1,
            Some(false) => self.effort_requested += 1,
            None => {}
        }
    }
}

/// One (pipeline, Node) slot of the by_model tree: the Node's rows under the
/// model buckets it provably ran on.
#[derive(Debug, Default)]
pub struct ModelAxisNodeAcc {
    pub name: String,
    /// #810 — the same genre the `by_pipeline` Node row carries, so the kind
    /// filter reads identically on both axes.
    pub kind: NodeKind,
    pub pairs: BTreeMap<ModelEffortKey, ModelPairAcc>,
    pub tallies: PipelineTallies,
}

/// One pipeline level of the by_model tree.
#[derive(Debug, Default)]
pub struct ModelAxisPipelineAcc {
    pub name: String,
    pub pairs: BTreeMap<ModelEffortKey, ModelPairAcc>,
    pub nodes: BTreeMap<String, ModelAxisNodeAcc>,
}

/// One effort level under a model. `effort: None` is the "not set" bucket.
#[derive(Debug, Default)]
pub struct ModelAxisEffortAcc {
    pub pairs: BTreeMap<ModelEffortKey, ModelPairAcc>,
    pub pipelines: BTreeMap<String, ModelAxisPipelineAcc>,
    /// Which raw efforts' Runs landed here (#906), by effort key.
    pub tallies: PipelineTallies,
}

/// One model (verbatim id) of the by_model axis.
#[derive(Debug, Default)]
pub struct ModelAxisModelAcc {
    pub pairs: BTreeMap<ModelEffortKey, ModelPairAcc>,
    pub efforts: BTreeMap<Option<String>, ModelAxisEffortAcc>,
    /// Which raw model ids landed here, and where each was read from (#892).
    pub tallies: PipelineTallies,
}

/// Attribute one session file's observation to every model × effort bucket its
/// identity names (ADR-0065 §3: the peak follows the file; a file naming two
/// models counts in both). Walks the by_model tree — Node, Pipeline, Effort
/// and Model levels each keep their own raw pool — plus the Node's own couples
/// on the `by_pipeline` side.
#[allow(clippy::too_many_arguments)]
pub fn record_model_observation(
    model_axis: &mut BTreeMap<String, ModelAxisModelAcc>,
    node_pairs: &mut BTreeMap<ModelEffortKey, ModelPairAcc>,
    pipeline: &RunIdentity,
    node: &NodeIdentity,
    node_kind: NodeKind,
    harness: &str,
    identity: &ModelIdentity,
    context: (Option<f64>, Option<&str>),
    durations: DurationReading<'_>,
    steering: Reading<'_>,
) {
    let key = (identity.model.clone(), identity.effort.clone());
    // #906: the Node's couple goes through the global absorptions, then the
    // couple absorptions of its raw Node; the « By model » axis below stops
    // before that last step.
    let couple = pipeline.couple(
        Some(&node.raw_key),
        &identity.model,
        identity.effort.as_deref(),
    );
    let node_pair = node_pairs
        .entry((couple.model.clone(), couple.effort.clone()))
        .or_default();
    node_pair.observe(harness, context, durations, steering, identity);
    node_pair
        .tallies
        .record_run_as(&couple.global_key, &pipeline.run_id, &pipeline.started_at);
    if couple.global {
        node_pair.global_tallies.record_run_as(
            &couple.raw_key,
            &pipeline.run_id,
            &pipeline.started_at,
        );
    }
    node_pair.scopes.insert(couple.scope);

    // #892: an absorbed model id counts under its absorbent on this axis only;
    // its efforts meet the absorbent's by effort.
    let model_acc = model_axis
        .entry(pipeline.model(&identity.model))
        .or_default();
    model_acc
        .tallies
        .record_run_as(&identity.model, &pipeline.run_id, &pipeline.started_at);
    model_acc
        .tallies
        .record_provenance_as(&identity.model, identity.model_observed);
    // #906: an effort absorption resolves on the RAW model id.
    let effort_acc = model_acc
        .efforts
        .entry(pipeline.effort(&identity.model, identity.effort.as_deref()))
        .or_default();
    effort_acc.tallies.record_run_as(
        &effort_key(identity.effort.as_deref()),
        &pipeline.run_id,
        &pipeline.started_at,
    );
    let pipeline_acc = effort_acc
        .pipelines
        .entry(pipeline.pipeline_key.clone())
        .or_default();
    pipeline.adopt_name(&mut pipeline_acc.name);
    let node_acc = pipeline_acc.nodes.entry(node.key.clone()).or_default();
    node.adopt_name(&mut node_acc.name);
    node_acc
        .tallies
        .record_run_as(&node.raw_key, &pipeline.run_id, &pipeline.started_at);
    node_acc.kind = node_kind;
    for acc in [
        model_acc.pairs.entry(key.clone()).or_default(),
        effort_acc.pairs.entry(key.clone()).or_default(),
        pipeline_acc.pairs.entry(key.clone()).or_default(),
        node_acc.pairs.entry(key).or_default(),
    ] {
        acc.observe(harness, context, durations, steering, identity);
    }
}

/// Pool a set of model × effort buckets back into one raw-observation
/// aggregate — the level rows of the by_model tree, never a mean of means.
pub fn pool_pairs(pairs: &BTreeMap<ModelEffortKey, ModelPairAcc>) -> PerformanceAggregate {
    let mut by_harness: BTreeMap<String, HarnessAcc> = BTreeMap::new();
    for pair in pairs.values() {
        for (harness, acc) in &pair.by_harness {
            let target = by_harness.entry(harness.clone()).or_default();
            target.context.fold(&acc.context);
            target.duration.fold(&acc.duration);
            target.active_duration.fold(&acc.active_duration);
            target.wait_duration.fold(&acc.wait_duration);
            target.steering.fold(&acc.steering);
        }
    }
    PerformanceAggregate {
        harnesses: finish_by_harness(by_harness),
    }
}

/// Observed/requested totals over a set of buckets — the provenance a level
/// row reports.
pub fn provenance_totals(pairs: &BTreeMap<ModelEffortKey, ModelPairAcc>) -> (i64, i64, i64, i64) {
    pairs.values().fold((0, 0, 0, 0), |(mo, mr, eo, er), acc| {
        (
            mo + acc.model_observed,
            mr + acc.model_requested,
            eo + acc.effort_observed,
            er + acc.effort_requested,
        )
    })
}

pub fn wire_model_pairs(
    pairs: BTreeMap<ModelEffortKey, ModelPairAcc>,
    resolver: &AbsorptionResolver,
) -> Vec<PerformanceModelEffortPair> {
    pairs
        .into_iter()
        .map(|((model, effort), acc)| {
            let key = couple_key(&model, effort.as_deref());
            PerformanceModelEffortPair {
                absorbed: resolver.absorbed_couples(&acc.scopes, &key, &acc.tallies),
                global_absorbed: acc.global_tallies.global_members(),
                runs: acc.tallies.runs(),
                last_run: acc.tallies.last_run(),
                key,
                model_provenance: provenance(acc.model_observed, acc.model_requested),
                effort_provenance: effort
                    .as_ref()
                    .map(|_| provenance(acc.effort_observed, acc.effort_requested)),
                aggregate: PerformanceAggregate {
                    harnesses: finish_by_harness(acc.by_harness),
                },
                model,
                effort,
            }
        })
        .collect()
}

/// Wire the whole Performance « By model » tree: Model → Effort → Pipeline →
/// Node. Every level's aggregate pools its own raw observations; the Node rows
/// carry no `models` — the path is already the drill.
pub fn wire_model_axis(
    models: BTreeMap<String, ModelAxisModelAcc>,
    resolver: &AbsorptionResolver,
) -> Vec<StatsModelPerformanceEntity> {
    models
        .into_iter()
        .map(|(model, model_acc)| {
            let (model_observed, model_requested, _, _) = provenance_totals(&model_acc.pairs);
            let efforts = model_acc
                .efforts
                .into_iter()
                .map(|(effort, effort_acc)| {
                    let (_, _, effort_observed, effort_requested) =
                        provenance_totals(&effort_acc.pairs);
                    let effort_provenance = effort
                        .as_ref()
                        .map(|_| provenance(effort_observed, effort_requested));
                    let pipelines = effort_acc
                        .pipelines
                        .into_iter()
                        .map(|(id, pipeline_acc)| {
                            let nodes = pipeline_acc
                                .nodes
                                .into_iter()
                                .map(|(node_id, node_acc)| PerformanceEntity {
                                    runs: Some(node_acc.tallies.runs()),
                                    last_run: node_acc.tallies.last_run(),
                                    absorbed: resolver.absorbed_nodes(
                                        &id,
                                        &node_id,
                                        &node_acc.tallies,
                                    ),
                                    id: node_id,
                                    name: node_acc.name,
                                    harnesses: pool_pairs(&node_acc.pairs).harnesses,
                                    nodes: Vec::new(),
                                    subagents: Vec::new(),
                                    interactive: Some(node_acc.kind.interactive),
                                    orchestrator: Some(node_acc.kind.orchestrator),
                                    models: Vec::new(),
                                })
                                .collect();
                            PerformanceEntity {
                                id,
                                name: pipeline_acc.name,
                                harnesses: pool_pairs(&pipeline_acc.pairs).harnesses,
                                nodes,
                                subagents: Vec::new(),
                                interactive: None,
                                orchestrator: None,
                                models: Vec::new(),
                                ..Default::default()
                            }
                        })
                        .collect();
                    let absorbed = resolver.absorbed_efforts(
                        model_acc.tallies.keys(),
                        &effort_key(effort.as_deref()),
                        &effort_acc.tallies,
                    );
                    PerformanceEffortEntity {
                        entity: PerformanceEntity {
                            runs: Some(effort_acc.tallies.runs()),
                            last_run: effort_acc.tallies.last_run(),
                            absorbed,
                            id: effort.clone().unwrap_or_default(),
                            name: effort.clone().unwrap_or_else(|| "not set".to_string()),
                            harnesses: pool_pairs(&effort_acc.pairs).harnesses,
                            nodes: Vec::new(),
                            subagents: Vec::new(),
                            interactive: None,
                            orchestrator: None,
                            models: Vec::new(),
                        },
                        effort,
                        provenance: effort_provenance,
                        pipelines,
                    }
                })
                .collect();
            StatsModelPerformanceEntity {
                entity: PerformanceEntity {
                    runs: Some(model_acc.tallies.runs()),
                    last_run: model_acc.tallies.last_run(),
                    absorbed: resolver.absorbed_models(&model, &model_acc.tallies),
                    id: model.clone(),
                    name: model,
                    harnesses: pool_pairs(&model_acc.pairs).harnesses,
                    nodes: Vec::new(),
                    subagents: Vec::new(),
                    interactive: None,
                    orchestrator: None,
                    models: Vec::new(),
                },
                provenance: provenance(model_observed, model_requested),
                efforts,
            }
        })
        .collect()
}

// --- Node identity/type resolution ---------------------------------------------
