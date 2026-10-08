//! A region re-entered by an edge: the next lap, or what its exhaustion routes to.

use super::*;

/// The bounded region's **effective** iteration cap: the live
/// `set_region_max_iter` override if the operator raised it in flight, else the
/// declared `max_iter` resolved from the pipeline. Read the override off
/// `run_state` (folded from the append-only log), never the YAML: that is what
/// makes the raise uniform across a literal and a `$var` cap, and durable across a
/// reopen re-projection.
pub fn effective_region_max_iter(
    run_state: &RunState,
    region: &pdo_pipeline::domain::pipeline::LoopRegion,
    resolved_vars: &HashMap<String, serde_yaml::Value>,
) -> i64 {
    run_state
        .region_max_iter_overrides
        .get(region.id.as_str())
        .copied()
        .unwrap_or_else(|| {
            crate::domain::loop_region::resolve_region_max_iter(region, resolved_vars)
        })
}

pub(super) fn handle_region_reentry(
    pipeline: &PipelineDef,
    run_state: &RunState,
    region: &pdo_pipeline::domain::pipeline::LoopRegion,
    entry_id: &str,
    source_iter: i64,
    frontmatter_fields: &HashMap<String, serde_yaml::Value>,
    resolved_vars: &HashMap<String, serde_yaml::Value>,
) -> Vec<SchedulerAction> {
    let mut actions = Vec::new();

    let max_iter = effective_region_max_iter(run_state, region, resolved_vars);
    let region_loop_state = run_state.loop_states.get(region.id.as_str());
    let current_iter = region_loop_state.map(|ls| ls.current_iter).unwrap_or(1);
    // An ended region (`end_region` projected as `done`) never starts another lap —
    // it routes its exit at the current iter, like exhaustion. A `force_route` on
    // the region is likewise an exit-now request: stop looping rather than run out
    // the (possibly just-raised) cap.
    let forced_region_route = run_state.forced_routes.get(region.id.as_str()).cloned();
    let ended = region_loop_state.is_some_and(|ls| ls.done) || forced_region_route.is_some();

    // Drop a back-edge from a member that lags the region counter
    // (`current_iter > source_iter`): it represents a lap the region ALREADY took.
    // `reopen_run` re-fires the outgoing edges of *every* settled-complete member
    // in one pass, so a lagging member would spawn the entry a lap ahead of the
    // frontier, forking the loop into two concurrent branches over the same
    // worktree. The frontier member's forward edge drives the legitimate resume.
    // An `ended`/force-routed region still routes its exit — terminal routing, not
    // a lap advance — so the guard scopes to the plain NextLap path only.
    if !ended && forced_region_route.is_none() && current_iter > source_iter {
        return actions;
    }

    let runtime = crate::domain::loop_region::RegionRuntime {
        current_iter,
        max_iter,
        exhausted: false,
    };

    // A fired back-edge means at least one re-entry was requested this lap.
    let decision = if ended {
        crate::domain::loop_region::LapDecision::Exhausted
    } else {
        crate::domain::loop_region::resolve_lap(pipeline, region, &runtime, 1)
    };
    match decision {
        crate::domain::loop_region::LapDecision::NextLap { iter, entry } => {
            actions.push(SchedulerAction::LoopIterStarted {
                loop_node_id: region.id.clone(),
                iter,
                max_iter,
            });
            let entry = if entry.is_empty() {
                entry_id.to_string()
            } else {
                entry
            };
            actions.push(SchedulerAction::Spawn {
                node_id: entry,
                iter,
            });
        }
        crate::domain::loop_region::LapDecision::Exhausted => {
            // #600: a `force_route` on the region overrides the `when:`-based
            // exhaustion routing — spawn the forced target (or complete, if it is
            // `End`), never the "exhausted — unrouted" park. This is the region
            // twin of the node-scoped force route in `evaluate_outgoing_edges_full`.
            if let Some(target) = forced_region_route {
                let end_node_id = pipeline
                    .nodes
                    .iter()
                    .find(|n| n.node_type == NodeType::End)
                    .map(|n| n.id.clone());
                if end_node_id.as_deref() == Some(target.as_str()) {
                    actions.push(SchedulerAction::Complete);
                } else {
                    actions.push(SchedulerAction::Spawn {
                        node_id: target,
                        iter: 1,
                    });
                }
                return actions;
            }
            match crate::domain::loop_region::exhaustion_outcome(
                pipeline,
                region,
                &runtime,
                frontmatter_fields,
                resolved_vars,
            ) {
                crate::domain::loop_region::ExhaustionOutcome::Routed(targets) => {
                    let end_node_id = pipeline
                        .nodes
                        .iter()
                        .find(|n| n.node_type == NodeType::End)
                        .map(|n| n.id.as_str());
                    for target in targets {
                        if end_node_id == Some(target.as_str()) {
                            actions.push(SchedulerAction::Complete);
                        } else {
                            actions.push(SchedulerAction::Spawn {
                                node_id: target,
                                iter: 1,
                            });
                        }
                    }
                }
                crate::domain::loop_region::ExhaustionOutcome::Unrouted => {
                    // Only the just-completed member's frontmatter is in scope, so
                    // read values only appear where the guard reads one of its fields.
                    let exit_edges = describe_region_exit_edges(
                        pipeline,
                        region,
                        current_iter,
                        frontmatter_fields,
                        resolved_vars,
                    );
                    let (reason_code, head) = if ended {
                        (
                            "region_ended_unrouted",
                            format!(
                                "ended — unrouted: bounded region '{}' was closed by end_region \
                                 at iter {current_iter} but no exit edge matched",
                                region.id
                            ),
                        )
                    } else {
                        (
                            "region_exhausted",
                            format!(
                                "exhausted — unrouted: bounded region '{}' reached max_iter \
                                 {max_iter} with the continuation condition still true and no \
                                 matching exit edge",
                                region.id
                            ),
                        )
                    };
                    let message = format!(
                        "{head}. Exit edges:\n{exit_edges}\n\
                         Raise the cap with set_region_max_iter, or route the exit with \
                         force_route (source '{}' -> a target).",
                        region.id
                    );
                    actions.push(SchedulerAction::Interrupt {
                        reason_code: reason_code.to_string(),
                        message,
                    });
                }
            }
        }
        crate::domain::loop_region::LapDecision::NoReentry => {}
    }

    actions
}

/// Decides the iter for a generic forward spawn of `target_id` after `source_id`
/// completed — or `None` when the target must not spawn:
///
/// - never run → iter 1;
/// - fed by an on-demand node and already launched → never re-run (ADR-0086);
/// - already ran → re-run ONLY when the fired edge closes an emergent cycle (the
///   target reaches the source through forward edges), at `iter + 1`. Re-spawning
///   a node reached only by forward edges is the "feeder dragged into a lap" bug;
/// - a bounded-region member is never spawned past its effective `max_iter`;
/// - a pure self-edge is inert outside a region.
pub(super) fn forward_spawn_iter(
    pipeline: &PipelineDef,
    run_state: &RunState,
    source_id: &str,
    target_id: &str,
    resolved_vars: &HashMap<String, serde_yaml::Value>,
) -> Option<i64> {
    if source_id == target_id {
        return None;
    }

    let proposed = match run_state.nodes.get(target_id) {
        None => 1,
        // ADR-0086: an on-demand node serves a downstream not launched yet; a
        // downstream already launched is not relaunched — Retry on it is the
        // explicit gesture, a cascade down to `End` is not.
        Some(_)
            if pipeline
                .nodes
                .iter()
                .any(|n| n.id == source_id && n.on_demand) =>
        {
            return None;
        }
        Some(ts) => {
            if reaches(pipeline, target_id, source_id) {
                ts.iter + 1
            } else {
                return None;
            }
        }
    };

    let member_region = crate::domain::loop_region::bounded_region_for_member(pipeline, target_id);
    if let Some(region) = member_region {
        // Honour a live `set_region_max_iter` raise here too: on the YAML bound,
        // the region head would re-enter but its body node would refuse the new lap.
        let max = effective_region_max_iter(run_state, region, resolved_vars);
        if proposed > max {
            return None;
        }

        // A member→member forward edge lifts the target to the source's lap. Fire
        // only while the target is genuinely a lap behind: `reopen_run` re-fires
        // every completed member's edges in one pass, and a target that already
        // caught up would be re-spawned a lap ahead of the source's output.
        if region.members.iter().any(|m| m == source_id) {
            let source_iter = run_state.nodes.get(source_id).map(|n| n.iter).unwrap_or(1);
            if run_state
                .nodes
                .get(target_id)
                .is_some_and(|ts| ts.iter >= source_iter)
            {
                return None;
            }
        }
    }

    Some(proposed)
}

/// True when a directed path of forward edges leads from `from` to `to`
/// (self-edges excluded: a node does not reach itself through its own edge).
pub(super) fn reaches(pipeline: &PipelineDef, from: &str, to: &str) -> bool {
    let mut visited: HashSet<&str> = HashSet::new();
    let mut queue: Vec<&str> = vec![from];
    while let Some(current) = queue.pop() {
        for edge in &pipeline.edges {
            if edge.source.node != current || edge.target.node == current {
                continue;
            }
            let next = edge.target.node.as_str();
            if next == to {
                return true;
            }
            if visited.insert(next) {
                queue.push(next);
            }
        }
    }
    false
}
