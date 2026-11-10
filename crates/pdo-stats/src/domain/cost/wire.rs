//! The hierarchies as the wire carries them.

use std::collections::BTreeMap;

use super::metric::CostAggregateAcc;
use super::pairs::{cost_then_id, wire_pairs, CostEntityAcc, ModelAcc};
use super::roll_up::wire_roll_up;
use super::{
    StatsCostEntity, StatsCostPeriod, StatsEffortCostEntity, StatsModelCostEntity,
    StatsProjectCostEntity,
};
use crate::domain::absorption::effort_key;
use crate::domain::absorption::resolver::AbsorptionResolver;
use crate::domain::provenance::provenance;

/// Wire the whole « By model » tree: Model → Effort → Pipeline → Node, ranked by
/// cost at every level. Every level carries `by_period` so the harness-stacked
/// bars follow the drill.
pub fn wire_by_model(
    models: BTreeMap<String, ModelAcc>,
    resolver: &AbsorptionResolver,
) -> Vec<StatsModelCostEntity> {
    let mut rows: Vec<StatsModelCostEntity> = models
        .into_iter()
        .map(|(model, acc)| {
            let mut efforts: Vec<StatsEffortCostEntity> = acc
                .efforts
                .into_iter()
                .map(|(effort, effort_acc)| {
                    let mut pipelines: Vec<StatsCostEntity> = effort_acc
                        .pipelines
                        .into_iter()
                        .map(|(id, pipeline_acc)| {
                            let mut nodes: Vec<StatsCostEntity> = pipeline_acc
                                .nodes
                                .into_iter()
                                .map(|(node_id, node)| wire_cost_node(&id, node_id, node, resolver))
                                .collect();
                            let mut entity = wire_cost_pipeline(id, pipeline_acc.entity, resolver);
                            nodes.sort_by(|a, b| {
                                cost_then_id(a.aggregate.usd, &a.id, b.aggregate.usd, &b.id)
                            });
                            entity.nodes = nodes;
                            entity
                        })
                        .collect();
                    pipelines.sort_by(|a, b| {
                        cost_then_id(a.aggregate.usd, &a.id, b.aggregate.usd, &b.id)
                    });
                    StatsEffortCostEntity {
                        entity: StatsCostEntity {
                            id: effort.clone().unwrap_or_default(),
                            name: effort.clone().unwrap_or_else(|| "not set".to_string()),
                            aggregate: effort_acc.aggregate.wire(),
                            by_period: wire_periods(effort_acc.periods),
                            nodes: Vec::new(),
                            models: Vec::new(),
                            runs: Some(effort_acc.tallies.runs()),
                            last_run: effort_acc.tallies.last_run(),
                            absorbed: resolver.absorbed_efforts(
                                acc.tallies.keys(),
                                &effort_key(effort.as_deref()),
                                &effort_acc.tallies,
                            ),
                            roll_up: None,
                        },
                        provenance: effort.as_ref().map(|_| {
                            provenance(effort_acc.effort_observed, effort_acc.effort_requested)
                        }),
                        effort,
                        pipelines,
                    }
                })
                .collect();
            efforts.sort_by(|a, b| {
                cost_then_id(
                    a.entity.aggregate.usd,
                    &a.entity.id,
                    b.entity.aggregate.usd,
                    &b.entity.id,
                )
            });
            StatsModelCostEntity {
                entity: StatsCostEntity {
                    absorbed: resolver.absorbed_models(&model, &acc.tallies),
                    runs: Some(acc.tallies.runs()),
                    last_run: acc.tallies.last_run(),
                    id: model.clone(),
                    name: model,
                    aggregate: acc.aggregate.wire(),
                    by_period: wire_periods(acc.periods),
                    nodes: Vec::new(),
                    models: Vec::new(),
                    roll_up: None,
                },
                provenance: provenance(acc.model_observed, acc.model_requested),
                efforts,
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        cost_then_id(
            a.entity.aggregate.usd,
            &a.entity.id,
            b.entity.aggregate.usd,
            &b.entity.id,
        )
    });
    rows
}

#[derive(Debug, Clone, Default)]
pub struct CostProjectAcc {
    pub name: String,
    pub aggregate: CostAggregateAcc,
    pub periods: BTreeMap<String, CostAggregateAcc>,
    pub pipelines: BTreeMap<String, CostEntityAcc>,
}

pub fn wire_periods(periods: BTreeMap<String, CostAggregateAcc>) -> Vec<StatsCostPeriod> {
    periods
        .into_iter()
        .map(|(bucket, aggregate)| StatsCostPeriod {
            bucket,
            aggregate: aggregate.wire(),
        })
        .collect()
}

pub fn wire_cost_entity(
    id: String,
    entity: CostEntityAcc,
    resolver: &AbsorptionResolver,
) -> StatsCostEntity {
    let mut nodes: Vec<_> = entity
        .nodes
        .into_iter()
        .map(|(id, node)| wire_cost_entity(id, node, resolver))
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
        models: wire_pairs(entity.pairs, resolver),
        runs: None,
        last_run: None,
        absorbed: Vec::new(),
        roll_up: None,
    }
}

/// Wire a level of Pipeline rows, ranked by cost then id.
pub fn wire_cost_pipelines(
    pipelines: BTreeMap<String, CostEntityAcc>,
    resolver: &AbsorptionResolver,
) -> Vec<StatsCostEntity> {
    let mut rows: Vec<StatsCostEntity> = pipelines
        .into_iter()
        .map(|(id, pipeline)| wire_cost_pipeline(id, pipeline, resolver))
        .collect();
    rows.sort_by(|a, b| cost_then_id(a.aggregate.usd, &a.id, b.aggregate.usd, &b.id));
    rows
}

/// Wire the Project rows, each with its Pipeline rows, ranked by cost then id.
pub fn wire_cost_projects(
    projects: BTreeMap<String, CostProjectAcc>,
    resolver: &AbsorptionResolver,
) -> Vec<StatsProjectCostEntity> {
    let mut rows: Vec<StatsProjectCostEntity> = projects
        .into_iter()
        .map(|(id, project)| StatsProjectCostEntity {
            entity: StatsCostEntity {
                id,
                name: project.name,
                aggregate: project.aggregate.wire(),
                by_period: wire_periods(project.periods),
                nodes: Vec::new(),
                models: Vec::new(),
                runs: None,
                last_run: None,
                absorbed: Vec::new(),
                roll_up: None,
            },
            pipelines: wire_cost_pipelines(project.pipelines, resolver),
        })
        .collect();
    rows.sort_by(|a, b| {
        cost_then_id(
            a.entity.aggregate.usd,
            &a.entity.id,
            b.entity.aggregate.usd,
            &b.entity.id,
        )
    });
    rows
}

/// A Pipeline row: the entity, plus what its Runs and its absorbed members say
/// — and the same for each of its Node rows (#892), and its roll-up.
pub fn wire_cost_pipeline(
    id: String,
    mut entity: CostEntityAcc,
    resolver: &AbsorptionResolver,
) -> StatsCostEntity {
    let runs = entity.tallies.runs();
    let last_run = entity.tallies.last_run();
    let absorbed = resolver.absorbed(&id, &entity.tallies);
    let roll_up = entity
        .roll_up
        .take()
        .and_then(|roll_up| wire_roll_up(&id, *roll_up, resolver));
    let mut nodes: Vec<StatsCostEntity> = std::mem::take(&mut entity.nodes)
        .into_iter()
        .map(|(node_id, node)| wire_cost_node(&id, node_id, node, resolver))
        .collect();
    nodes.sort_by(|a, b| cost_then_id(a.aggregate.usd, &a.id, b.aggregate.usd, &b.id));
    let mut wired = wire_cost_entity(id, entity, resolver);
    wired.nodes = nodes;
    wired.runs = Some(runs);
    wired.last_run = last_run;
    wired.absorbed = absorbed;
    wired.roll_up = roll_up.map(Box::new);
    wired
}

/// A Node row under Pipeline row `pipeline`: its Runs and its absorbed Nodes.
/// The Infrastructure and Unassigned rows are no Node: they record no Run.
pub fn wire_cost_node(
    pipeline: &str,
    id: String,
    entity: CostEntityAcc,
    resolver: &AbsorptionResolver,
) -> StatsCostEntity {
    let runs = entity.tallies.runs();
    let last_run = entity.tallies.last_run();
    let absorbed = resolver.absorbed_nodes(pipeline, &id, &entity.tallies);
    let mut wired = wire_cost_entity(id, entity, resolver);
    if runs > 0 {
        wired.runs = Some(runs);
        wired.last_run = last_run;
    }
    wired.absorbed = absorbed;
    wired
}
