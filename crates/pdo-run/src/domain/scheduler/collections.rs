//! A `kind: collection` region: its fan-out, its barrier, and what a member
//! waits on upstream.

use super::*;

/// Drives the ENTRY of a `kind: collection` region (ADR-0011) when an external
/// edge fired into it. An empty collection still emits `CollectionEmpty` +
/// `CollectionDone` and fires the barrier targets, so a vacuous region never
/// stalls the run.
///
/// Idempotent per region: once `collection_states[region.id]` exists the fan-out
/// already happened, and a second inbound edge re-firing must not double the laps.
pub(super) fn handle_collection_entry(
    pipeline: &PipelineDef,
    run_state: &RunState,
    region: &pdo_pipeline::domain::pipeline::LoopRegion,
    frontmatter_fields: &HashMap<String, serde_yaml::Value>,
    frontmatter_by_node: &HashMap<String, HashMap<String, serde_yaml::Value>>,
    vars: &HashMap<String, serde_yaml::Value>,
) -> Vec<SchedulerAction> {
    let mut actions = Vec::new();

    if run_state.collection_states.contains_key(region.id.as_str()) {
        return actions;
    }

    let fanout =
        crate::domain::loop_region::collection_fanout(pipeline, region, frontmatter_fields);

    if fanout.total == 0 {
        actions.push(SchedulerAction::CollectionEmpty {
            region_id: region.id.clone(),
        });
        actions.push(SchedulerAction::CollectionDone {
            region_id: region.id.clone(),
        });
        actions.extend(collection_barrier_spawns(
            pipeline,
            run_state,
            region,
            frontmatter_by_node,
            vars,
        ));
        return actions;
    }

    actions.push(SchedulerAction::CollectionStarted {
        region_id: region.id.clone(),
        entry: fanout.entry.clone(),
        members: region.members.clone(),
        total_items: fanout.total,
        items: fanout.items,
    });
    for i in 1..=fanout.total {
        actions.push(SchedulerAction::Spawn {
            node_id: fanout.entry.clone(),
            iter: i,
        });
    }

    actions
}

/// Evaluates the BARRIER of a `kind: collection` region (ADR-0011): once every
/// member has completed every lap `1..=total`, emit `CollectionDone` and fire the
/// region's exits once. Must be called on freshly projected state.
///
/// An exit target fed by other edges too is a convergence (ADR-0082): the
/// barrier resolves the region's edges into it, and the target starts only once
/// its other incoming edges are resolved as well. `frontmatter_by_node` (every
/// completed producer's frontmatter) and `vars` are what tell those edges
/// resolved; the edge that resolves last spawns it.
pub fn evaluate_collection_barrier(
    pipeline: &PipelineDef,
    run_state: &RunState,
    region: &pdo_pipeline::domain::pipeline::LoopRegion,
    frontmatter_by_node: &HashMap<String, HashMap<String, serde_yaml::Value>>,
    vars: &HashMap<String, serde_yaml::Value>,
) -> Vec<SchedulerAction> {
    let mut actions = Vec::new();

    let collection_state = match run_state.collection_states.get(region.id.as_str()) {
        Some(cs) if !cs.done => cs,
        _ => return actions,
    };

    let total = collection_state.total_items;
    let completed_iters: HashSet<i64> = (1..=total)
        .filter(|i| {
            region.members.iter().all(|member| {
                run_state.nodes.get(member.as_str()).is_some_and(|n| {
                    n.iterations
                        .iter()
                        .any(|it| it.iter == *i && it.status.is_settled_complete())
                })
            })
        })
        .collect();

    if !crate::domain::loop_region::collection_barrier_reached(total, &completed_iters) {
        return actions;
    }

    actions.push(SchedulerAction::CollectionDone {
        region_id: region.id.clone(),
    });
    actions.extend(collection_barrier_spawns(
        pipeline,
        run_state,
        region,
        frontmatter_by_node,
        vars,
    ));

    actions
}

/// What the barrier of `region` fires: its exit targets whose every other
/// incoming edge is resolved, with the region's own edges resolved by the
/// barrier itself ([`upstream_resolved`]). A target still waiting on another
/// branch — a bounded loop that has not exited, a producer still running — is
/// left to that branch: its routing spawns the target once the region reads
/// `done`.
pub(super) fn collection_barrier_spawns(
    pipeline: &PipelineDef,
    run_state: &RunState,
    region: &pdo_pipeline::domain::pipeline::LoopRegion,
    frontmatter_by_node: &HashMap<String, HashMap<String, serde_yaml::Value>>,
    vars: &HashMap<String, serde_yaml::Value>,
) -> Vec<SchedulerAction> {
    let end_node_id = pipeline
        .nodes
        .iter()
        .find(|n| n.node_type == NodeType::End)
        .map(|n| n.id.as_str());
    crate::domain::loop_region::collection_barrier_targets(pipeline, region)
        .into_iter()
        .filter(|target| {
            upstream_resolved(
                pipeline,
                run_state,
                target,
                None,
                Some(region.id.as_str()),
                frontmatter_by_node,
                vars,
            )
        })
        .map(|target| {
            if end_node_id == Some(target.as_str()) {
                SchedulerAction::Complete
            } else {
                SchedulerAction::Spawn {
                    node_id: target,
                    iter: 1,
                }
            }
        })
        .collect()
}

pub(super) fn check_all_upstream_completed(
    pipeline: &PipelineDef,
    run_state: &RunState,
    target_node_id: &str,
    just_completed_node_id: &str,
    frontmatter_by_node: &HashMap<String, HashMap<String, serde_yaml::Value>>,
    vars: &HashMap<String, serde_yaml::Value>,
) -> bool {
    upstream_resolved(
        pipeline,
        run_state,
        target_node_id,
        Some(just_completed_node_id),
        None,
        frontmatter_by_node,
        vars,
    )
}

/// Is every incoming edge of `target_node_id` resolved (ADR-0082) — fired, or
/// dead? `just_completed` is the producer whose completion is being routed (its
/// edge is the one firing); `settled_collection` is a collection region whose
/// barrier is firing now, its `done` not projected yet.
fn upstream_resolved(
    pipeline: &PipelineDef,
    run_state: &RunState,
    target_node_id: &str,
    just_completed: Option<&str>,
    settled_collection: Option<&str>,
    frontmatter_by_node: &HashMap<String, HashMap<String, serde_yaml::Value>>,
    vars: &HashMap<String, serde_yaml::Value>,
) -> bool {
    // Forward preconditions only: a self-edge can never be satisfied before the
    // node's own first run, and a bounded-region back-edge (member -> entry)
    // belongs to `handle_region_reentry`. Counting either as an upstream blocker
    // makes the join unsatisfiable and stalls the run silently.
    let upstream: HashSet<&str> = pipeline
        .edges
        .iter()
        .filter(|e| e.target.node == target_node_id)
        .filter(|e| e.source.node != target_node_id)
        // ADR-0086: an on-demand node not launched yet holds no barrier.
        .filter(|e| crate::domain::on_demand::edge_counts(pipeline, run_state, e))
        .filter(|e| {
            crate::domain::loop_region::bounded_region_reentered_by_edge(
                pipeline,
                &e.source.node,
                target_node_id,
            )
            .is_none()
        })
        .map(|e| e.source.node.as_str())
        .collect();

    upstream.iter().all(|src| {
        // A collection-region member upstream is a BARRIER input: it counts as
        // completed only once the whole region is done. A per-lap completion (or a
        // stale `Completed` status mid-fan-out) must not satisfy the join early.
        if let Some(region) =
            crate::domain::loop_region::collection_region_for_member(pipeline, src)
        {
            return settled_collection == Some(region.id.as_str())
                || run_state
                    .collection_states
                    .get(region.id.as_str())
                    .is_some_and(|cs| cs.done);
        }
        if just_completed == Some(*src) {
            return true;
        }
        if run_state
            .nodes
            .get(*src)
            .is_some_and(|n| n.status.is_settled_complete())
        {
            // A bounded-loop member settled on an intermediate lap whose edge
            // here did not fire is not resolved: a later lap can still fire it.
            return !loop_exit_pending(
                pipeline,
                run_state,
                src,
                target_node_id,
                frontmatter_by_node,
                vars,
            );
        }
        // ADR-0011 ("jamais de stall silencieux"): a convergence target (any
        // node fed by several edges) must not wait forever on an upstream branch that is dead — a
        // non-firing conditional/`else` edge, or a transitively-dead producer.
        // Such a branch never appears in `run_state` and never completes, so we
        // treat its edge as resolved rather than a blocker.
        let mut visiting = HashSet::new();
        is_node_dead(
            pipeline,
            run_state,
            src,
            frontmatter_by_node,
            vars,
            &mut visiting,
        )
    })
}
