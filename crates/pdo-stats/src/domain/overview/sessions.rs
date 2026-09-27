//! The Sessions fold: every agentic `node_started` of the Runs started in the
//! window, counted per period, per harness and per Pipeline → Node row.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use pdo_run::application::ports::NodeStart;
use pdo_run::domain::run_cost::frozen_execution_identity;

use super::{
    BucketCount, HarnessCount, StatsSessionEntity, StatsSessionHarness, StatsSessionPeriod,
};
use crate::domain::absorption::resolver::AbsorptionResolver;
use crate::domain::absorption::tallies::PipelineTallies;

#[derive(Debug, Clone)]
pub struct SessionNodeBuilder {
    pub name: String,
    pub count: HarnessCount,
    pub periods: BTreeMap<String, HarnessCount>,
    /// Which raw Node ids' executions landed here (#892).
    pub tallies: PipelineTallies,
}

impl Default for SessionNodeBuilder {
    fn default() -> Self {
        Self {
            name: String::new(),
            count: HarnessCount::default(),
            periods: BTreeMap::new(),
            tallies: PipelineTallies::counting_executions(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionPipelineBuilder {
    pub name: String,
    pub count: HarnessCount,
    pub periods: BTreeMap<String, HarnessCount>,
    pub nodes: BTreeMap<String, SessionNodeBuilder>,
    pub tallies: PipelineTallies,
}

impl Default for SessionPipelineBuilder {
    fn default() -> Self {
        Self {
            name: String::new(),
            count: HarnessCount::default(),
            periods: BTreeMap::new(),
            nodes: BTreeMap::new(),
            tallies: PipelineTallies::counting_executions(),
        }
    }
}

pub fn increment_harness(count: &mut HarnessCount, harness: &str) {
    count.total += 1;
    *count.by_harness.entry(harness.to_string()).or_default() += 1;
}

pub fn harness_rows(count: BTreeMap<String, u64>) -> Vec<StatsSessionHarness> {
    count
        .into_iter()
        .map(|(harness, executions)| StatsSessionHarness {
            harness,
            executions,
        })
        .collect()
}

pub fn period_rows(periods: BTreeMap<String, HarnessCount>) -> Vec<StatsSessionPeriod> {
    periods
        .into_iter()
        .map(|(bucket, count)| StatsSessionPeriod {
            bucket,
            harnesses: harness_rows(count.by_harness),
        })
        .collect()
}

pub fn finish_node(
    pipeline: &str,
    id: String,
    builder: SessionNodeBuilder,
    resolver: &AbsorptionResolver,
) -> StatsSessionEntity {
    let absorbed = resolver.absorbed_nodes(pipeline, &id, &builder.tallies);
    StatsSessionEntity {
        id,
        name: builder.name,
        executions: builder.count.total,
        harnesses: harness_rows(builder.count.by_harness),
        by_period: period_rows(builder.periods),
        nodes: Vec::new(),
        runs: Some(builder.tallies.runs()),
        last_run: builder.tallies.last_run(),
        absorbed,
    }
}

pub fn finish_pipeline(
    id: String,
    builder: SessionPipelineBuilder,
    resolver: &AbsorptionResolver,
) -> StatsSessionEntity {
    let mut nodes: Vec<StatsSessionEntity> = builder
        .nodes
        .into_iter()
        .map(|(node_id, node)| finish_node(&id, node_id, node, resolver))
        .collect();
    nodes.sort_by(|a, b| {
        b.executions
            .cmp(&a.executions)
            .then_with(|| a.id.cmp(&b.id))
    });
    let absorbed = resolver.absorbed(&id, &builder.tallies);
    StatsSessionEntity {
        id,
        name: builder.name,
        executions: builder.count.total,
        harnesses: harness_rows(builder.count.by_harness),
        by_period: period_rows(builder.periods),
        nodes,
        runs: Some(builder.tallies.runs()),
        last_run: builder.tallies.last_run(),
        absorbed,
    }
}

pub struct SessionStats {
    pub sessions: Vec<BucketCount>,
    pub periods: Vec<StatsSessionPeriod>,
    pub harnesses: Vec<String>,
    pub pipelines: Vec<StatsSessionEntity>,
}

/// Fold the Node starts of the window's cohort (in the log's order: by the
/// Run's start, then the event's id) into the Sessions series.
pub fn session_stats(rows: Vec<NodeStart>, resolver: &AbsorptionResolver) -> SessionStats {
    let mut periods = BTreeMap::<String, HarnessCount>::new();
    let mut session_periods = BTreeMap::<String, i64>::new();
    let mut harnesses = BTreeSet::<String>::new();
    let mut pipelines = BTreeMap::<String, SessionPipelineBuilder>::new();
    let mut seen_executions = HashSet::new();

    for (order, row) in rows.into_iter().enumerate() {
        let NodeStart {
            bucket,
            run_ts: started_at,
            run_id,
            run_payload,
            event_id,
            node_id,
            iter,
            payload: event_payload,
        } = row;
        let identity = resolver.run_identity(&run_payload);

        let Some(node_id) = node_id else {
            continue;
        };
        let node_type = event_payload
            .get("node_type")
            .and_then(|value| value.as_str())
            .or_else(|| {
                identity
                    .node_defs
                    .get(&node_id)
                    .map(|def| def.node_type.as_str())
            })
            .unwrap_or("");
        if node_type == "script" {
            continue;
        }
        let harness = event_payload
            .get("harness")
            .and_then(|value| value.as_str())
            .filter(|harness| !harness.is_empty())
            .unwrap_or("claude")
            .to_string();
        let execution = frozen_execution_identity(
            &harness,
            event_payload
                .get("session_id")
                .and_then(|value| value.as_str()),
            Some(&node_id),
            iter,
            event_id,
            order,
        );
        if !seen_executions.insert((run_id.clone(), execution)) {
            continue;
        }

        harnesses.insert(harness.clone());
        increment_harness(periods.entry(bucket.clone()).or_default(), &harness);

        let pipeline = pipelines.entry(identity.pipeline_key.clone()).or_default();
        identity.adopt_name(&mut pipeline.name);
        pipeline.tallies.record_run(&identity, &run_id, &started_at);
        pipeline.tallies.record_execution(&identity);
        increment_harness(&mut pipeline.count, &harness);
        increment_harness(
            pipeline.periods.entry(bucket.clone()).or_default(),
            &harness,
        );

        *session_periods.entry(bucket.clone()).or_default() += 1;
        // #892: a Node absorbed under this Pipeline row counts under its
        // absorbent Node.
        let node_identity = identity.node(&node_id);
        let node = pipeline.nodes.entry(node_identity.key.clone()).or_default();
        node_identity.adopt_name(&mut node.name);
        node.tallies
            .record_run_as(&node_identity.raw_key, &run_id, &started_at);
        node.tallies.record_execution_as(&node_identity.raw_key);
        increment_harness(&mut node.count, &harness);
        increment_harness(node.periods.entry(bucket).or_default(), &harness);
    }

    let mut pipeline_rows: Vec<_> = pipelines
        .into_iter()
        .map(|(id, pipeline)| finish_pipeline(id, pipeline, resolver))
        .collect();
    pipeline_rows.sort_by(|a, b| {
        b.executions
            .cmp(&a.executions)
            .then_with(|| a.id.cmp(&b.id))
    });

    SessionStats {
        sessions: session_periods
            .into_iter()
            .map(|(bucket, count)| BucketCount { bucket, count })
            .collect(),
        periods: period_rows(periods),
        harnesses: harnesses.into_iter().collect(),
        pipelines: pipeline_rows,
    }
}
