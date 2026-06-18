use std::collections::{HashMap, HashSet};

use crate::condition;
use crate::edge_router;
use crate::event_log::{NodeStatus, RunState};
use crate::graph_resolver;
use crate::pipeline::{NodeType, PipelineDef};
use crate::switch_router;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulerAction {
    Spawn {
        node_id: String,
        iter: i64,
    },
    Halt {
        message: String,
    },
    Complete,
    SwitchRouted {
        node_id: String,
        chosen_branch: String,
    },
    LoopIterStarted {
        loop_node_id: String,
        iter: i64,
        max_iter: i64,
    },
    LoopBreakReceived {
        loop_node_id: String,
    },
    LoopMaxReached {
        loop_node_id: String,
        max_iter: i64,
    },
    LoopDone {
        loop_node_id: String,
    },
    ForEachStarted {
        foreach_node_id: String,
        total_items: i64,
        items: Vec<serde_yaml::Value>,
    },
    ForEachEmpty {
        foreach_node_id: String,
    },
    ForEachBreakReceived {
        foreach_node_id: String,
    },
    ForEachDone {
        foreach_node_id: String,
    },
}

/// Bootstraps Loop nodes whose `in` port is fed by a Start node (or a node
/// already completed) but whose first iteration has not yet been started.
///
/// Returns a list of `LoopIterStarted{1}` plus `Spawn{body_target, 1}` actions
/// for each such loop. The caller is responsible for emitting the events and
/// spawning the body subgraph entry nodes.
///
/// This closes the gap between [`ready_nodes`] (which deliberately skips Loop
/// nodes — they are not spawnable as tmux sessions) and the regular outgoing
/// edge handling in [`evaluate_outgoing_edges_with_context`] (which never
/// fires when the loop is the very first downstream of `Start`, because Start
/// itself never "completes" in the scheduler's eyes).
pub fn seed_pending_loops(
    pipeline: &PipelineDef,
    run_state: &RunState,
    resolved_vars: &HashMap<String, serde_yaml::Value>,
) -> Vec<SchedulerAction> {
    let mut actions = Vec::new();

    for loop_node in pipeline
        .nodes
        .iter()
        .filter(|n| n.node_type == NodeType::Loop)
    {
        if run_state.loop_states.contains_key(&loop_node.id) {
            continue;
        }

        let in_edges: Vec<_> = pipeline
            .edges
            .iter()
            .filter(|e| e.target.node == loop_node.id && e.target.port == "in")
            .collect();
        if in_edges.is_empty() {
            continue;
        }

        let any_satisfied = in_edges.iter().any(|edge| {
            let src = &edge.source.node;
            let is_start = pipeline
                .nodes
                .iter()
                .any(|n| n.id == *src && n.node_type == NodeType::Start);
            if is_start {
                return true;
            }
            run_state
                .nodes
                .get(src.as_str())
                .is_some_and(|ns| ns.status == NodeStatus::Completed)
        });
        if !any_satisfied {
            continue;
        }

        actions.push(SchedulerAction::LoopIterStarted {
            loop_node_id: loop_node.id.clone(),
            iter: 1,
            max_iter: resolve_max_iter(loop_node, resolved_vars),
        });
        for edge in &pipeline.edges {
            if edge.source.node == loop_node.id && edge.source.port == "body" {
                actions.push(SchedulerAction::Spawn {
                    node_id: edge.target.node.clone(),
                    iter: 1,
                });
            }
        }
    }

    actions
}

#[cfg(test)]
pub fn evaluate_outgoing_edges(
    pipeline: &PipelineDef,
    run_state: &RunState,
    completed_node_id: &str,
) -> Vec<SchedulerAction> {
    evaluate_outgoing_edges_with_context(
        pipeline,
        run_state,
        completed_node_id,
        &HashMap::new(),
        &HashMap::new(),
    )
}

pub fn evaluate_outgoing_edges_with_context(
    pipeline: &PipelineDef,
    run_state: &RunState,
    completed_node_id: &str,
    resolved_vars: &HashMap<String, serde_yaml::Value>,
    frontmatter_fields: &HashMap<String, serde_yaml::Value>,
) -> Vec<SchedulerAction> {
    // Single-node callers (tests, var-update reprocessing) supply only the
    // just-completed node's frontmatter. Seed the per-node map with it so
    // convergence suppression can re-evaluate this producer's edges. Other
    // producers fall back to empty frontmatter (treated as live — conservative).
    let mut frontmatter_by_node: HashMap<String, HashMap<String, serde_yaml::Value>> =
        HashMap::new();
    frontmatter_by_node.insert(completed_node_id.to_string(), frontmatter_fields.clone());
    evaluate_outgoing_edges_full(
        pipeline,
        run_state,
        completed_node_id,
        resolved_vars,
        frontmatter_fields,
        &frontmatter_by_node,
    )
}

/// Same as [`evaluate_outgoing_edges_with_context`] but with an explicit
/// per-node frontmatter map, so convergence suppression (ADR-0011) can
/// re-evaluate the conditional edges of *other* completed producers (e.g. the
/// classifier feeding a suppressed `else` branch). This is THE canonical
/// scheduler entry point: the daemon's event-driven handlers
/// (`handle_node_completion`, `re_evaluate_after_command`) call it for each
/// completed producer.
pub fn evaluate_outgoing_edges_full(
    pipeline: &PipelineDef,
    run_state: &RunState,
    completed_node_id: &str,
    resolved_vars: &HashMap<String, serde_yaml::Value>,
    frontmatter_fields: &HashMap<String, serde_yaml::Value>,
    frontmatter_by_node: &HashMap<String, HashMap<String, serde_yaml::Value>>,
) -> Vec<SchedulerAction> {
    let mut actions = Vec::new();

    let source_iter = run_state
        .nodes
        .get(completed_node_id)
        .map(|n| n.iter)
        .unwrap_or(1);

    let completed_node = pipeline.nodes.iter().find(|n| n.id == completed_node_id);
    let is_switch = completed_node.is_some_and(|n| n.node_type == NodeType::Switch);

    let matched_port = if is_switch {
        let switch_node = completed_node.unwrap();
        let chosen =
            switch_router::route(switch_node, frontmatter_fields, resolved_vars, source_iter)
                .to_string();
        actions.push(SchedulerAction::SwitchRouted {
            node_id: completed_node_id.to_string(),
            chosen_branch: chosen.clone(),
        });
        Some(chosen)
    } else {
        None
    };

    let end_node_id = pipeline
        .nodes
        .iter()
        .find(|n| n.node_type == NodeType::End)
        .map(|n| n.id.as_str());

    // Conditional routing on edges (ADR-0011): for a non-Switch producer,
    // evaluate the source node's outgoing edges in multi-match. Every edge whose
    // `when:` is satisfied fires; an `else` edge fires iff no sibling on the same
    // source port matched; an unconditional edge always fires. We compute the
    // firing set up-front (keyed by index into `pipeline.edges`) and gate the
    // loop on it. (Switch nodes keep their own port-based routing via
    // `matched_port` for backward compatibility.)
    let fired_indices: HashSet<usize> = if is_switch {
        // Switch routing is handled by `matched_port`; don't double-gate.
        HashSet::new()
    } else {
        let outgoing: Vec<(usize, &crate::pipeline::EdgeDef)> = pipeline
            .edges
            .iter()
            .enumerate()
            .filter(|(_, e)| e.source.node == completed_node_id)
            .collect();
        let edge_refs: Vec<&crate::pipeline::EdgeDef> = outgoing.iter().map(|(_, e)| *e).collect();
        let fired =
            edge_router::fired_edges(&edge_refs, frontmatter_fields, resolved_vars, source_iter);
        // Map firing edges back to their global indices by identity.
        outgoing
            .iter()
            .filter(|(_, e)| fired.iter().any(|f| std::ptr::eq(*f, *e)))
            .map(|(i, _)| *i)
            .collect()
    };

    for (edge_index, edge) in pipeline.edges.iter().enumerate() {
        if edge.source.node != completed_node_id {
            continue;
        }

        if let Some(ref port) = matched_port {
            if edge.source.port != *port {
                continue;
            }
        }

        // Skip edges whose conditional clause did not fire (non-Switch sources).
        if !is_switch && !fired_indices.contains(&edge_index) {
            continue;
        }

        let target_id = &edge.target.node;

        if end_node_id == Some(target_id.as_str()) {
            if let Some(raw_msg) = edge.reason.as_deref() {
                let rendered = condition::render_halt_message(
                    raw_msg,
                    &condition::HaltContext {
                        iter: source_iter,
                        node_id: completed_node_id.to_string(),
                        variables: resolved_vars.clone(),
                        fields: frontmatter_fields.clone(),
                    },
                );
                actions.push(SchedulerAction::Halt { message: rendered });
            } else {
                actions.push(SchedulerAction::Complete);
            }
        } else if let Some(region) = crate::loop_region::bounded_region_reentered_by_edge(
            pipeline,
            completed_node_id,
            target_id,
        ) {
            // ── Bounded loop REGION re-entry (ADR-0011 / #148) ────────────────
            //
            // The fired edge is a region back-edge (member -> entry): the
            // region wants another lap. The region engine — not the generic
            // forward-spawn path — governs this. While iter < max, advance the
            // counter and re-spawn the entry once (coalesced). At max_iter with
            // re-entry still requested, the region is *exhausted*: route an
            // `iter >= max` exit edge if one matches, else emit the explicit
            // "exhausted — unrouted" halt (never a silent stall, never an
            // off-by-one spawn past the bound).
            actions.extend(handle_region_reentry(
                pipeline,
                run_state,
                region,
                target_id,
                frontmatter_fields,
                resolved_vars,
            ));
        } else {
            let target_node = pipeline.nodes.iter().find(|n| n.id == *target_id);
            let is_loop_target = target_node.is_some_and(|n| n.node_type == NodeType::Loop);
            let is_foreach_target = target_node.is_some_and(|n| n.node_type == NodeType::ForEach);

            let is_switch_target = target_node.is_some_and(|n| n.node_type == NodeType::Switch);

            if is_switch_target {
                let all_upstream_done = check_all_upstream_completed(
                    pipeline,
                    run_state,
                    target_id,
                    completed_node_id,
                    frontmatter_by_node,
                    resolved_vars,
                );
                if all_upstream_done {
                    let switch_actions = evaluate_outgoing_edges_with_context(
                        pipeline,
                        run_state,
                        target_id,
                        resolved_vars,
                        frontmatter_fields,
                    );
                    actions.extend(switch_actions);
                }
            } else if is_loop_target {
                let loop_actions = handle_loop_input(
                    pipeline,
                    run_state,
                    target_id,
                    &edge.target.port,
                    resolved_vars,
                );
                actions.extend(loop_actions);
            } else if is_foreach_target {
                let foreach_actions = handle_foreach_input(
                    pipeline,
                    run_state,
                    target_id,
                    &edge.target.port,
                    frontmatter_fields,
                );
                actions.extend(foreach_actions);
            } else {
                let all_upstream_done = check_all_upstream_completed(
                    pipeline,
                    run_state,
                    target_id,
                    completed_node_id,
                    frontmatter_by_node,
                    resolved_vars,
                );

                if all_upstream_done {
                    if let Some(next_iter) = forward_spawn_iter(
                        pipeline,
                        run_state,
                        completed_node_id,
                        target_id,
                        resolved_vars,
                    ) {
                        actions.push(SchedulerAction::Spawn {
                            node_id: target_id.clone(),
                            iter: next_iter,
                        });
                    }
                }
            }
        }
    }

    // ── Explicit halt on unrouted convergence (ADR-0011, "jamais de stall
    // silencieux", extended to Merge by the ADR-0006 addendum) ───────────────
    //
    // The edge-resolution barrier above lets a convergence node spawn on its
    // *live* (fired) branches and skip *dead* (permanently-suppressed) ones. But
    // a convergence whose branches are ALL dead never has an edge fire into it,
    // so it is never spawned — it becomes a dead node too. Death propagates
    // downstream: when the cascade renders `End` unreachable through every live
    // path, the Run would otherwise sit `Running` forever. Detect that here and
    // emit an explicit Halt instead, so the state is diagnosable ("unrouted")
    // rather than a silent stall.
    //
    // We only consider halting when this completion produced no forward progress
    // (no Spawn / Complete / Halt). If End is still reachable through any live
    // path, `is_node_dead(End)` is false and we stay our hand — a Merge waiting
    // on a still-running sibling is normal, not a stall.
    if !is_switch
        && !actions.iter().any(|a| {
            matches!(
                a,
                SchedulerAction::Spawn { .. }
                    | SchedulerAction::Complete
                    | SchedulerAction::Halt { .. }
            )
        })
    {
        if let Some(end_id) = end_node_id {
            let mut visiting = HashSet::new();
            let end_dead = is_node_dead(
                pipeline,
                run_state,
                end_id,
                frontmatter_by_node,
                resolved_vars,
                &mut visiting,
            );
            if end_dead {
                actions.push(SchedulerAction::Halt {
                    message: "unrouted: conditional routing suppressed every path to End \
                         (no live branch reaches End)"
                        .to_string(),
                });
            }
        }
    }

    actions
}

/// Drives one re-entry of a bounded loop region (ADR-0011 / #148) when a member's
/// back-edge fired. Delegates the decision to the pure region engine
/// (`loop_region`):
///
/// - **NextLap**: emit `LoopIterStarted{region, iter+1}` (so the projection
///   tracks the region counter in `loop_states`) and re-`Spawn` the region entry
///   once at the next iter — even if several back-edges fired this lap, the
///   engine coalesces to one (#108).
/// - **Exhausted** (`iter >= max_iter` with re-entry still requested): consult
///   `exhaustion_outcome`. `Routed` ⇒ `Spawn` each external target (or `Complete`
///   if it is `End`); `Unrouted` ⇒ the explicit "exhausted — unrouted" `Halt` —
///   the diagnosable state the Pipeline Manager routes (#152), never a silent
///   stall and never an off-by-one spawn past the bound.
///
/// The region's live counter is read from `run_state.loop_states[region.id]`,
/// defaulting to lap 1 before any iteration event has been projected.
fn handle_region_reentry(
    pipeline: &PipelineDef,
    run_state: &RunState,
    region: &crate::pipeline::LoopRegion,
    entry_id: &str,
    frontmatter_fields: &HashMap<String, serde_yaml::Value>,
    resolved_vars: &HashMap<String, serde_yaml::Value>,
) -> Vec<SchedulerAction> {
    let mut actions = Vec::new();

    let max_iter = crate::loop_region::resolve_region_max_iter(region, resolved_vars);
    let region_loop_state = run_state.loop_states.get(region.id.as_str());
    let current_iter = region_loop_state.map(|ls| ls.current_iter).unwrap_or(1);
    // #199: an ended region (`end_region` projected as `done`) never starts
    // another lap — it routes its exit at the current iter, like exhaustion.
    let ended = region_loop_state.is_some_and(|ls| ls.done);

    let runtime = crate::loop_region::RegionRuntime {
        current_iter,
        max_iter,
        exhausted: false,
    };

    // A fired back-edge means at least one re-entry was requested this lap.
    let decision = if ended {
        crate::loop_region::LapDecision::Exhausted
    } else {
        crate::loop_region::resolve_lap(pipeline, region, &runtime, 1)
    };
    match decision {
        crate::loop_region::LapDecision::NextLap { iter, entry } => {
            actions.push(SchedulerAction::LoopIterStarted {
                loop_node_id: region.id.clone(),
                iter,
                max_iter,
            });
            // Trust the engine's entry, falling back to the edge target.
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
        crate::loop_region::LapDecision::Exhausted => {
            match crate::loop_region::exhaustion_outcome(
                pipeline,
                region,
                &runtime,
                frontmatter_fields,
                resolved_vars,
            ) {
                crate::loop_region::ExhaustionOutcome::Routed(targets) => {
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
                crate::loop_region::ExhaustionOutcome::Unrouted => {
                    let message = if ended {
                        format!(
                            "ended — unrouted: bounded region '{}' was closed by end_region \
                             at iter {current_iter} but no exit edge matched (route it from \
                             the Pipeline Manager)",
                            region.id
                        )
                    } else {
                        format!(
                            "exhausted — unrouted: bounded region '{}' reached max_iter \
                             {max_iter} with the continuation condition still true and no \
                             matching exit edge (route it from the Pipeline Manager)",
                            region.id
                        )
                    };
                    actions.push(SchedulerAction::Halt { message });
                }
            }
        }
        crate::loop_region::LapDecision::NoReentry => {}
    }

    actions
}

/// Decides the iter for a generic forward spawn of `target_id` after
/// `source_id` completed — or `None` when the target must not spawn
/// (#199 / #195 / #210):
///
/// - never run → spawn at iter 1;
/// - already ran → re-run ONLY when the fired edge closes an emergent cycle
///   (the target reaches the source through forward edges), at `iter + 1`.
///   A node reached only by forward edges is never re-spawned by
///   re-evaluation — that is the "feeder dragged into a lap" bug;
/// - a bounded-region member is never spawned past its effective `max_iter`;
/// - a pure self-edge (source == target) is inert outside a region (#207).
fn forward_spawn_iter(
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
        Some(ts) => {
            if reaches(pipeline, target_id, source_id) {
                ts.iter + 1
            } else {
                return None;
            }
        }
    };

    let member_region = pipeline.loops.iter().find(|r| {
        r.kind == crate::pipeline::LoopKind::Bounded && r.members.iter().any(|m| m == target_id)
    });
    if let Some(region) = member_region {
        let max = crate::loop_region::resolve_region_max_iter(region, resolved_vars);
        if proposed > max {
            return None;
        }
    }

    Some(proposed)
}

/// True when a directed path of forward edges leads from `from` to `to`
/// (self-edges excluded: a node does not reach itself through its own edge).
fn reaches(pipeline: &PipelineDef, from: &str, to: &str) -> bool {
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

pub fn resolve_max_iter(
    loop_node: &crate::pipeline::NodeDef,
    resolved_vars: &HashMap<String, serde_yaml::Value>,
) -> i64 {
    match &loop_node.max_iter {
        Some(serde_yaml::Value::Number(n)) => n.as_i64().unwrap_or(5),
        Some(serde_yaml::Value::String(s)) => {
            if let Some(var_name) = s.strip_prefix('$') {
                resolved_vars
                    .get(var_name)
                    .and_then(|v| v.as_i64())
                    .unwrap_or(5)
            } else {
                s.parse::<i64>().unwrap_or(5)
            }
        }
        _ => 5,
    }
}

fn handle_loop_input(
    pipeline: &PipelineDef,
    run_state: &RunState,
    loop_node_id: &str,
    target_port: &str,
    resolved_vars: &HashMap<String, serde_yaml::Value>,
) -> Vec<SchedulerAction> {
    let mut actions = Vec::new();

    let loop_node = match pipeline.nodes.iter().find(|n| n.id == loop_node_id) {
        Some(n) => n,
        None => return actions,
    };

    match target_port {
        "in" => {
            let iter = run_state
                .loop_states
                .get(loop_node_id)
                .map(|ls| ls.current_iter)
                .unwrap_or(1);

            actions.push(SchedulerAction::LoopIterStarted {
                loop_node_id: loop_node_id.to_string(),
                iter,
                max_iter: resolve_max_iter(loop_node, resolved_vars),
            });

            // Fire body subgraph entry nodes
            for edge in &pipeline.edges {
                if edge.source.node == loop_node_id && edge.source.port == "body" {
                    actions.push(SchedulerAction::Spawn {
                        node_id: edge.target.node.clone(),
                        iter,
                    });
                }
            }
        }
        "break" => {
            actions.push(SchedulerAction::LoopBreakReceived {
                loop_node_id: loop_node_id.to_string(),
            });
        }
        _ => {}
    }

    actions
}

pub fn evaluate_loop_body_completion(
    pipeline: &PipelineDef,
    run_state: &RunState,
    loop_node_id: &str,
    resolved_vars: &HashMap<String, serde_yaml::Value>,
) -> Vec<SchedulerAction> {
    let mut actions = Vec::new();

    let loop_node = match pipeline.nodes.iter().find(|n| n.id == loop_node_id) {
        Some(n) if n.node_type == NodeType::Loop => n,
        _ => return actions,
    };

    let loop_state = match run_state.loop_states.get(loop_node_id) {
        Some(ls) => ls,
        None => return actions,
    };

    // Break is unconditional termination — skip body completion check.
    if loop_state.break_received {
        actions.push(SchedulerAction::LoopDone {
            loop_node_id: loop_node_id.to_string(),
        });
        fire_done_port(pipeline, loop_node_id, &mut actions);
        return actions;
    }

    let body_nodes = match graph_resolver::compute_body_subgraph(pipeline, loop_node_id) {
        Ok(nodes) => nodes,
        Err(_) => return actions,
    };

    let current_iter = loop_state.current_iter;

    let all_body_done = body_nodes.iter().all(|node_id| {
        run_state
            .nodes
            .get(node_id)
            .is_some_and(|n| n.status == NodeStatus::Completed && n.iter >= current_iter)
    });

    if !all_body_done {
        return actions;
    }

    let max_iter = resolve_max_iter(loop_node, resolved_vars);

    if current_iter >= max_iter {
        actions.push(SchedulerAction::LoopMaxReached {
            loop_node_id: loop_node_id.to_string(),
            max_iter,
        });
        actions.push(SchedulerAction::LoopDone {
            loop_node_id: loop_node_id.to_string(),
        });
        fire_done_port(pipeline, loop_node_id, &mut actions);
    } else {
        let next_iter = current_iter + 1;
        actions.push(SchedulerAction::LoopIterStarted {
            loop_node_id: loop_node_id.to_string(),
            iter: next_iter,
            max_iter,
        });

        // Re-fire body subgraph entry nodes
        for edge in &pipeline.edges {
            if edge.source.node == loop_node_id && edge.source.port == "body" {
                actions.push(SchedulerAction::Spawn {
                    node_id: edge.target.node.clone(),
                    iter: next_iter,
                });
            }
        }
    }

    actions
}

fn fire_done_port(pipeline: &PipelineDef, loop_node_id: &str, actions: &mut Vec<SchedulerAction>) {
    for edge in &pipeline.edges {
        if edge.source.node == loop_node_id && edge.source.port == "done" {
            let target_id = &edge.target.node;
            let end_node_id = pipeline
                .nodes
                .iter()
                .find(|n| n.node_type == NodeType::End)
                .map(|n| n.id.as_str());

            if end_node_id == Some(target_id.as_str()) {
                actions.push(SchedulerAction::Complete);
            } else {
                actions.push(SchedulerAction::Spawn {
                    node_id: target_id.clone(),
                    iter: 1,
                });
            }
        }
    }
}

pub fn foreach_resolve_collection(
    frontmatter_fields: &HashMap<String, serde_yaml::Value>,
    over: &str,
) -> Vec<serde_yaml::Value> {
    frontmatter_fields
        .get(over)
        .and_then(|v| v.as_sequence())
        .cloned()
        .unwrap_or_default()
}

fn handle_foreach_input(
    pipeline: &PipelineDef,
    run_state: &RunState,
    foreach_node_id: &str,
    target_port: &str,
    frontmatter_fields: &HashMap<String, serde_yaml::Value>,
) -> Vec<SchedulerAction> {
    let mut actions = Vec::new();

    let foreach_node = match pipeline.nodes.iter().find(|n| n.id == foreach_node_id) {
        Some(n) => n,
        None => return actions,
    };

    match target_port {
        "in" => {
            if run_state.foreach_states.contains_key(foreach_node_id) {
                return actions;
            }

            let over_field = foreach_node.over.as_deref().unwrap_or("items");
            let items = foreach_resolve_collection(frontmatter_fields, over_field);

            let total = items.len() as i64;

            if total == 0 {
                actions.push(SchedulerAction::ForEachEmpty {
                    foreach_node_id: foreach_node_id.to_string(),
                });
                actions.push(SchedulerAction::ForEachDone {
                    foreach_node_id: foreach_node_id.to_string(),
                });
                for edge in &pipeline.edges {
                    if edge.source.node == foreach_node_id && edge.source.port == "done" {
                        let end_node_id = pipeline
                            .nodes
                            .iter()
                            .find(|n| n.node_type == NodeType::End)
                            .map(|n| n.id.as_str());
                        if end_node_id == Some(edge.target.node.as_str()) {
                            actions.push(SchedulerAction::Complete);
                        } else {
                            actions.push(SchedulerAction::Spawn {
                                node_id: edge.target.node.clone(),
                                iter: 1,
                            });
                        }
                    }
                }
                return actions;
            }

            actions.push(SchedulerAction::ForEachStarted {
                foreach_node_id: foreach_node_id.to_string(),
                total_items: total,
                items: items.clone(),
            });

            for i in 1..=total {
                for edge in &pipeline.edges {
                    if edge.source.node == foreach_node_id && edge.source.port == "body" {
                        actions.push(SchedulerAction::Spawn {
                            node_id: edge.target.node.clone(),
                            iter: i,
                        });
                    }
                }
            }
        }
        "break" => {
            actions.push(SchedulerAction::ForEachBreakReceived {
                foreach_node_id: foreach_node_id.to_string(),
            });
        }
        _ => {}
    }

    actions
}

pub fn evaluate_foreach_body_completion(
    pipeline: &PipelineDef,
    run_state: &RunState,
    foreach_node_id: &str,
) -> Vec<SchedulerAction> {
    let mut actions = Vec::new();

    let foreach_node = match pipeline.nodes.iter().find(|n| n.id == foreach_node_id) {
        Some(n) if n.node_type == NodeType::ForEach => n,
        _ => return actions,
    };

    let foreach_state = match run_state.foreach_states.get(foreach_node_id) {
        Some(fs) if !fs.done => fs,
        _ => return actions,
    };

    let body_nodes = match graph_resolver::compute_body_subgraph(pipeline, foreach_node_id) {
        Ok(nodes) => nodes,
        Err(_) => return actions,
    };

    let total = foreach_state.total_items;

    let all_iters_done = (1..=total).all(|i| {
        body_nodes.iter().all(|node_id| {
            run_state.nodes.get(node_id).is_some_and(|n| {
                n.iterations
                    .iter()
                    .any(|it| it.iter == i && it.status == NodeStatus::Completed)
            })
        })
    });

    if !all_iters_done {
        return actions;
    }

    actions.push(SchedulerAction::ForEachDone {
        foreach_node_id: foreach_node_id.to_string(),
    });

    for edge in &pipeline.edges {
        if edge.source.node == foreach_node.id && edge.source.port == "done" {
            let end_node_id = pipeline
                .nodes
                .iter()
                .find(|n| n.node_type == NodeType::End)
                .map(|n| n.id.as_str());

            if end_node_id == Some(edge.target.node.as_str()) {
                actions.push(SchedulerAction::Complete);
            } else {
                actions.push(SchedulerAction::Spawn {
                    node_id: edge.target.node.clone(),
                    iter: 1,
                });
            }
        }
    }

    actions
}

fn check_all_upstream_completed(
    pipeline: &PipelineDef,
    run_state: &RunState,
    target_node_id: &str,
    just_completed_node_id: &str,
    frontmatter_by_node: &HashMap<String, HashMap<String, serde_yaml::Value>>,
    vars: &HashMap<String, serde_yaml::Value>,
) -> bool {
    // Forward preconditions only (#194 / #210, preserving #172): a self-edge can
    // never be satisfied before the node's own first run, and a bounded-region
    // back-edge (member -> entry) is the region engine's concern
    // (`handle_region_reentry`) — counting either as an upstream blocker
    // makes the join unsatisfiable and stalls the run silently. Two forensic
    // sources: #172 (entering a bounded loop from an external forward edge — the
    // entry never spawned because its back-edge source sits downstream in the
    // cycle), and run 9c8d123 (#194 — the loop-entry node never spawned, zero
    // events for 8+ min). The sprint's region-engine check
    // (`bounded_region_reentered_by_edge`) subsumes #172's edge-index exclusion
    // and additionally drops self-edges (#207); the #172 regression tests
    // (`external_forward_edge_spawns_bounded_loop_entry`,
    // `bounded_loop_entry_then_forwards_to_second_member`) still guard this path.
    let upstream: HashSet<&str> = pipeline
        .edges
        .iter()
        .filter(|e| e.target.node == target_node_id)
        .filter(|e| e.source.node != target_node_id)
        .filter(|e| {
            crate::loop_region::bounded_region_reentered_by_edge(
                pipeline,
                &e.source.node,
                target_node_id,
            )
            .is_none()
        })
        .map(|e| e.source.node.as_str())
        .collect();

    upstream.iter().all(|src| {
        if *src == just_completed_node_id {
            return true;
        }
        if run_state
            .nodes
            .get(*src)
            .is_some_and(|n| n.status == NodeStatus::Completed)
        {
            return true;
        }
        // ADR-0011 ("jamais de stall silencieux"): a convergence target (e.g. a
        // `Merge`) must not wait forever on an upstream branch that is dead — a
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

/// Returns `true` when `node_id` is **dead** for this run (decided model,
/// ADR-0006 addendum): it has incoming edges and every one of them is dead —
/// i.e. each producer has completed and the edge into `node_id` did not fire
/// (its `when:` was false, or it is an `else` whose sibling matched), or the
/// producer is itself dead. Death propagates upstream-to-downstream through this
/// recursion: a node fed only by dead branches is dead, including a `Merge`
/// whose `branches` are all dead, and including `End` itself (used to detect an
/// unrouted convergence that must halt explicitly rather than stall silently).
///
/// Conservative on purpose: if any incoming edge is still *live* (its producer
/// has not completed yet, or the edge fired, or the producer's outcome is not
/// yet decided), the node is NOT dead and the convergence keeps waiting. A node
/// already present in `run_state` (spawned at any status) is by definition not
/// dead. A node with no incoming edges is a root and is likewise never dead.
fn is_node_dead(
    pipeline: &PipelineDef,
    run_state: &RunState,
    node_id: &str,
    frontmatter_by_node: &HashMap<String, HashMap<String, serde_yaml::Value>>,
    vars: &HashMap<String, serde_yaml::Value>,
    visiting: &mut HashSet<String>,
) -> bool {
    // Already spawned (running / completed / failed / stopped): not dead.
    if run_state.nodes.contains_key(node_id) {
        return false;
    }
    // Cycle guard: if we re-encounter a node mid-walk, do not let it prop up its
    // own deadness. Treat the recursion as "not dead via this edge".
    if !visiting.insert(node_id.to_string()) {
        return false;
    }

    let incoming: Vec<&crate::pipeline::EdgeDef> = pipeline
        .edges
        .iter()
        .filter(|e| e.target.node == node_id)
        .collect();

    // A root with no incoming edges is an entry point, never dead.
    if incoming.is_empty() {
        visiting.remove(node_id);
        return false;
    }

    // The node is dead iff EVERY incoming edge is dead.
    let dead = incoming.iter().all(|edge| {
        let src = edge.source.node.as_str();
        let producer = pipeline.nodes.iter().find(|n| n.id == src);
        let producer_completed = run_state
            .nodes
            .get(src)
            .is_some_and(|n| n.status == NodeStatus::Completed);

        if producer_completed {
            // The producer has run: this edge is dead only if it did NOT fire.
            // Recompute the firing set from the producer's recorded frontmatter.
            // Switch producers route by port; we conservatively treat their
            // edges as live (Switch is being retired by ADR-0011 and is not part
            // of the conditional-edge convergence path).
            let is_switch = producer.is_some_and(|n| n.node_type == NodeType::Switch);
            if is_switch {
                return false; // live: keep waiting
            }
            let source_iter = run_state.nodes.get(src).map(|n| n.iter).unwrap_or(1);
            let empty = HashMap::new();
            let fm = frontmatter_by_node.get(src).unwrap_or(&empty);
            let outgoing: Vec<&crate::pipeline::EdgeDef> = pipeline
                .edges
                .iter()
                .filter(|e| e.source.node == src)
                .collect();
            let fired = edge_router::fired_edges(&outgoing, fm, vars, source_iter);
            let this_edge_fired = fired.iter().any(|f| std::ptr::eq(*f, *edge));
            // Dead iff this edge did not fire.
            !this_edge_fired
        } else if run_state.nodes.contains_key(src) {
            // Producer spawned but not completed (running / awaiting / failed):
            // outcome not yet decided — edge is still live.
            false
        } else {
            // Producer never spawned: this edge is dead only if the producer is
            // itself dead (recurse).
            is_node_dead(
                pipeline,
                run_state,
                src,
                frontmatter_by_node,
                vars,
                visiting,
            )
        }
    });

    visiting.remove(node_id);
    dead
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_log::NodeState;
    use crate::graph_resolver::ready_nodes;
    use crate::pipeline::{EdgeDef, EdgeEndpoint, NodeDef, NodeType, Port, PortType};
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    fn make_node(id: &str, inputs: &[&str], outputs: &[&str]) -> NodeDef {
        NodeDef {
            id: id.into(),
            name: id.into(),
            node_type: NodeType::DocOnly,
            inputs: inputs
                .iter()
                .map(|n| Port {
                    name: (*n).into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                })
                .collect(),
            outputs: outputs
                .iter()
                .map(|n| Port {
                    name: (*n).into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                })
                .collect(),
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
        }
    }

    fn make_end_node() -> NodeDef {
        NodeDef {
            id: "end".into(),
            name: "End".into(),
            node_type: NodeType::End,
            inputs: vec![Port {
                name: "result".into(),
                repeated: false,
                side: None,
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
            }],
            outputs: vec![],
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
        }
    }

    fn make_edge(src_node: &str, src_port: &str, tgt_node: &str, tgt_port: &str) -> EdgeDef {
        EdgeDef {
            source: EdgeEndpoint {
                node: src_node.into(),
                port: src_port.into(),
            },
            target: EdgeEndpoint {
                node: tgt_node.into(),
                port: tgt_port.into(),
            },
            reason: None,
            when: None,
            is_else: false,
            repeated: false,
            ..Default::default()
        }
    }

    fn make_cond_edge(
        src_node: &str,
        src_port: &str,
        tgt_node: &str,
        tgt_port: &str,
        when: Option<&str>,
        is_else: bool,
    ) -> EdgeDef {
        EdgeDef {
            source: EdgeEndpoint {
                node: src_node.into(),
                port: src_port.into(),
            },
            target: EdgeEndpoint {
                node: tgt_node.into(),
                port: tgt_port.into(),
            },
            reason: None,
            when: when.map(|s| serde_yaml::from_str(s).unwrap()),
            is_else,
            repeated: false,
            ..Default::default()
        }
    }

    fn make_end_edge(src_node: &str, src_port: &str, reason: &str) -> EdgeDef {
        EdgeDef {
            source: EdgeEndpoint {
                node: src_node.into(),
                port: src_port.into(),
            },
            target: EdgeEndpoint {
                node: "end".into(),
                port: "result".into(),
            },
            reason: Some(reason.into()),
            when: None,
            is_else: false,
            repeated: false,
            ..Default::default()
        }
    }

    fn empty_run_state() -> RunState {
        RunState::new("run-1".into(), "test".into())
    }

    fn completed_node(id: &str) -> NodeState {
        NodeState {
            node_id: id.into(),
            status: NodeStatus::Completed,
            iter: 1,
            started_at: Some("t0".into()),
            completed_at: Some("t1".into()),
            failure_reason: None,
            iterations: Vec::new(),
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
        }
    }

    fn completed_node_iter(id: &str, iter: i64) -> NodeState {
        NodeState {
            node_id: id.into(),
            status: NodeStatus::Completed,
            iter,
            started_at: Some("t0".into()),
            completed_at: Some("t1".into()),
            failure_reason: None,
            iterations: Vec::new(),
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
        }
    }

    fn running_node(id: &str) -> NodeState {
        NodeState {
            node_id: id.into(),
            status: NodeStatus::Running,
            iter: 1,
            started_at: Some("t0".into()),
            completed_at: None,
            failure_reason: None,
            iterations: Vec::new(),
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
        }
    }

    // --- ready_nodes ---

    #[test]
    fn linear_chain_first_node_ready() {
        let pipeline = PipelineDef {
            name: "linear".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("planner", &["task"], &["plan"]),
                make_node("implementer", &["plan"], &["summary"]),
                make_node("reviewer", &["summary"], &["review"]),
            ],
            edges: vec![
                make_edge("planner", "plan", "implementer", "plan"),
                make_edge("implementer", "summary", "reviewer", "summary"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let state = empty_run_state();
        let ready = ready_nodes(&pipeline, &state);
        assert_eq!(ready, vec!["planner"]);
    }

    #[test]
    fn linear_chain_second_node_ready_after_first_completes() {
        let pipeline = PipelineDef {
            name: "linear".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("planner", &["task"], &["plan"]),
                make_node("implementer", &["plan"], &["summary"]),
                make_node("reviewer", &["summary"], &["review"]),
            ],
            edges: vec![
                make_edge("planner", "plan", "implementer", "plan"),
                make_edge("implementer", "summary", "reviewer", "summary"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("planner".into(), completed_node("planner"));
        let ready = ready_nodes(&pipeline, &state);
        assert_eq!(ready, vec!["implementer"]);
    }

    #[test]
    fn linear_chain_no_ready_while_running() {
        let pipeline = PipelineDef {
            name: "linear".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("planner", &["task"], &["plan"]),
                make_node("implementer", &["plan"], &["summary"]),
            ],
            edges: vec![make_edge("planner", "plan", "implementer", "plan")],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("planner".into(), running_node("planner"));
        let ready = ready_nodes(&pipeline, &state);
        assert!(ready.is_empty());
    }

    #[test]
    fn fan_out_both_children_ready() {
        let pipeline = PipelineDef {
            name: "fan-out".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("planner", &["task"], &["plan"]),
                make_node("impl-a", &["plan"], &["summary"]),
                make_node("impl-b", &["plan"], &["summary"]),
            ],
            edges: vec![
                make_edge("planner", "plan", "impl-a", "plan"),
                make_edge("planner", "plan", "impl-b", "plan"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("planner".into(), completed_node("planner"));
        let mut ready = ready_nodes(&pipeline, &state);
        ready.sort();
        assert_eq!(ready, vec!["impl-a", "impl-b"]);
    }

    #[test]
    fn fan_in_waits_for_all_parents() {
        let pipeline = PipelineDef {
            name: "fan-in".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("impl-a", &["task"], &["summary"]),
                make_node("impl-b", &["task"], &["summary"]),
                make_node("merger", &["summary-a", "summary-b"], &["merged"]),
            ],
            edges: vec![
                make_edge("impl-a", "summary", "merger", "summary-a"),
                make_edge("impl-b", "summary", "merger", "summary-b"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("impl-a".into(), completed_node("impl-a"));
        state.nodes.insert("impl-b".into(), running_node("impl-b"));
        let ready = ready_nodes(&pipeline, &state);
        assert!(ready.is_empty());

        state
            .nodes
            .insert("impl-b".into(), completed_node("impl-b"));
        let ready = ready_nodes(&pipeline, &state);
        assert_eq!(ready, vec!["merger"]);
    }

    #[test]
    fn partial_completion_next_ready_set() {
        let pipeline = PipelineDef {
            name: "diamond".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("a", &["task"], &["out"]),
                make_node("b", &["in"], &["out"]),
                make_node("c", &["in"], &["out"]),
                make_node("d", &["in-b", "in-c"], &["result"]),
            ],
            edges: vec![
                make_edge("a", "out", "b", "in"),
                make_edge("a", "out", "c", "in"),
                make_edge("b", "out", "d", "in-b"),
                make_edge("c", "out", "d", "in-c"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state.nodes.insert("a".into(), completed_node("a"));
        state.nodes.insert("b".into(), running_node("b"));

        let ready = ready_nodes(&pipeline, &state);
        assert_eq!(ready, vec!["c"]);
    }

    #[test]
    fn all_completed_returns_empty() {
        let pipeline = PipelineDef {
            name: "linear".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("a", &["task"], &["out"]),
                make_node("b", &["in"], &["out"]),
            ],
            edges: vec![make_edge("a", "out", "b", "in")],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state.nodes.insert("a".into(), completed_node("a"));
        state.nodes.insert("b".into(), completed_node("b"));

        let ready = ready_nodes(&pipeline, &state);
        assert!(ready.is_empty());
    }

    // --- evaluate_outgoing_edges ---

    #[test]
    fn unconditional_edge_spawns_target() {
        let pipeline = PipelineDef {
            name: "linear".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("a", &["task"], &["out"]),
                make_node("b", &["in"], &["out"]),
            ],
            edges: vec![make_edge("a", "out", "b", "in")],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state.nodes.insert("a".into(), completed_node("a"));

        let actions = evaluate_outgoing_edges(&pipeline, &state, "a");
        assert_eq!(
            actions,
            vec![SchedulerAction::Spawn {
                node_id: "b".into(),
                iter: 1,
            }]
        );
    }

    #[test]
    fn end_edge_produces_halt_action() {
        let pipeline = PipelineDef {
            name: "halt-test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("reviewer", &["code"], &["review"]),
                make_end_node(),
            ],
            edges: vec![make_end_edge(
                "reviewer",
                "review",
                "Blocked after {iter} iterations on {node-id}",
            )],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("reviewer".into(), completed_node_iter("reviewer", 3));

        let actions = evaluate_outgoing_edges(&pipeline, &state, "reviewer");
        assert_eq!(
            actions,
            vec![SchedulerAction::Halt {
                message: "Blocked after 3 iterations on reviewer".into(),
            }]
        );
    }

    #[test]
    fn end_edge_without_reason_produces_complete() {
        let pipeline = PipelineDef {
            name: "complete-test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("implementer", &["task"], &["summary"]),
                make_end_node(),
            ],
            edges: vec![EdgeDef {
                source: EdgeEndpoint {
                    node: "implementer".into(),
                    port: "summary".into(),
                },
                target: EdgeEndpoint {
                    node: "end".into(),
                    port: "result".into(),
                },
                reason: None,
                when: None,
                is_else: false,
                repeated: false,
                ..Default::default()
            }],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("implementer".into(), completed_node("implementer"));

        let actions = evaluate_outgoing_edges(&pipeline, &state, "implementer");
        assert_eq!(actions, vec![SchedulerAction::Complete]);
    }

    #[test]
    fn cycle_back_edge_increments_iter() {
        // reviewer completes at iter 2 → the back-edge of the emergent
        // implementer<->reviewer cycle fires → implementer already at iter 2,
        // so next spawn is iter 3. (#210: the forward edge implementer->
        // reviewer is part of the graph — only a real emergent cycle may
        // re-run a completed node; a forward-only feeder never is.)
        let pipeline = PipelineDef {
            name: "cycle".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("implementer", &["review"], &["code"]),
                make_node("reviewer", &["code"], &["review"]),
            ],
            edges: vec![
                make_edge("implementer", "code", "reviewer", "code"),
                make_edge("reviewer", "review", "implementer", "review"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("reviewer".into(), completed_node_iter("reviewer", 2));
        state
            .nodes
            .insert("implementer".into(), completed_node_iter("implementer", 2));

        let actions = evaluate_outgoing_edges(&pipeline, &state, "reviewer");
        assert_eq!(
            actions,
            vec![SchedulerAction::Spawn {
                node_id: "implementer".into(),
                iter: 3,
            }]
        );
    }

    #[test]
    fn multiple_outgoing_edges_can_fire_in_parallel() {
        let pipeline = PipelineDef {
            name: "fan-out".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("a", &["task"], &["out"]),
                make_node("b", &["in"], &["out"]),
                make_node("c", &["in"], &["out"]),
            ],
            edges: vec![
                make_edge("a", "out", "b", "in"),
                make_edge("a", "out", "c", "in"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state.nodes.insert("a".into(), completed_node("a"));

        let actions = evaluate_outgoing_edges(&pipeline, &state, "a");
        assert_eq!(actions.len(), 2);
        assert!(actions.contains(&SchedulerAction::Spawn {
            node_id: "b".into(),
            iter: 1,
        }));
        assert!(actions.contains(&SchedulerAction::Spawn {
            node_id: "c".into(),
            iter: 1,
        }));
    }

    #[test]
    fn conditional_edges_multi_match_spawn_all_satisfied_targets() {
        // ADR-0011: a producer fans out to ALL guarded edges whose `when:` is
        // satisfied; the `else` edge is suppressed because a sibling matched.
        let pipeline = PipelineDef {
            name: "cond-fanout".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("classifier", &["task"], &["triage"]),
                make_node("hotfix", &["triage"], &["patch"]),
                make_node("security", &["triage"], &["review"]),
                make_node("backlog", &["triage"], &["note"]),
            ],
            edges: vec![
                make_cond_edge(
                    "classifier",
                    "triage",
                    "hotfix",
                    "triage",
                    Some("severity: { eq: high }"),
                    false,
                ),
                make_cond_edge(
                    "classifier",
                    "triage",
                    "security",
                    "triage",
                    Some("security: { eq: true }"),
                    false,
                ),
                make_cond_edge("classifier", "triage", "backlog", "triage", None, true),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("classifier".into(), completed_node("classifier"));

        let fm: HashMap<String, serde_yaml::Value> = [
            ("severity".into(), serde_yaml::Value::String("high".into())),
            ("security".into(), serde_yaml::Value::Bool(true)),
        ]
        .into_iter()
        .collect();

        let actions = evaluate_outgoing_edges_with_context(
            &pipeline,
            &state,
            "classifier",
            &HashMap::new(),
            &fm,
        );

        assert!(actions.contains(&SchedulerAction::Spawn {
            node_id: "hotfix".into(),
            iter: 1,
        }));
        assert!(actions.contains(&SchedulerAction::Spawn {
            node_id: "security".into(),
            iter: 1,
        }));
        assert!(
            !actions.contains(&SchedulerAction::Spawn {
                node_id: "backlog".into(),
                iter: 1,
            }),
            "else edge must be suppressed when a sibling matched: {actions:?}"
        );
    }

    #[test]
    fn conditional_edges_else_fires_when_none_match() {
        let pipeline = PipelineDef {
            name: "cond-else".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("classifier", &["task"], &["triage"]),
                make_node("hotfix", &["triage"], &["patch"]),
                make_node("backlog", &["triage"], &["note"]),
            ],
            edges: vec![
                make_cond_edge(
                    "classifier",
                    "triage",
                    "hotfix",
                    "triage",
                    Some("severity: { eq: high }"),
                    false,
                ),
                make_cond_edge("classifier", "triage", "backlog", "triage", None, true),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("classifier".into(), completed_node("classifier"));

        let fm: HashMap<String, serde_yaml::Value> =
            [("severity".into(), serde_yaml::Value::String("low".into()))]
                .into_iter()
                .collect();

        let actions = evaluate_outgoing_edges_with_context(
            &pipeline,
            &state,
            "classifier",
            &HashMap::new(),
            &fm,
        );

        assert!(
            !actions.contains(&SchedulerAction::Spawn {
                node_id: "hotfix".into(),
                iter: 1,
            }),
            "unmatched guarded edge must not fire: {actions:?}"
        );
        assert!(
            actions.contains(&SchedulerAction::Spawn {
                node_id: "backlog".into(),
                iter: 1,
            }),
            "else edge must fire when no sibling matched: {actions:?}"
        );
    }

    fn make_merge_node(id: &str) -> NodeDef {
        let mut n = make_node(id, &["branches"], &["merged"]);
        n.node_type = NodeType::Merge;
        n
    }

    /// Regression for the L5 `conditional-edge-routing` stall (ADR-0011, #144):
    /// a `Merge` fed by three unconditional edges (hotfix, security-review,
    /// backlog) must NOT wait forever on `backlog`, which is permanently
    /// suppressed because its inbound `else` edge from `classifier` did not fire
    /// (a guarded sibling matched). "jamais de stall silencieux."
    fn fanout_merge_pipeline() -> PipelineDef {
        PipelineDef {
            name: "cond-merge".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("classifier", &["task"], &["triage"]),
                make_node("hotfix", &["triage"], &["patch"]),
                make_node("security", &["triage"], &["review"]),
                make_node("backlog", &["triage"], &["note"]),
                make_merge_node("merge1"),
            ],
            edges: vec![
                make_cond_edge(
                    "classifier",
                    "triage",
                    "hotfix",
                    "triage",
                    Some("severity: { eq: high }"),
                    false,
                ),
                make_cond_edge(
                    "classifier",
                    "triage",
                    "security",
                    "triage",
                    Some("security: { eq: true }"),
                    false,
                ),
                make_cond_edge("classifier", "triage", "backlog", "triage", None, true),
                make_edge("hotfix", "patch", "merge1", "branches"),
                make_edge("security", "review", "merge1", "branches"),
                make_edge("backlog", "note", "merge1", "branches"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        }
    }

    fn classifier_high_security_fm() -> HashMap<String, HashMap<String, serde_yaml::Value>> {
        let fm: HashMap<String, serde_yaml::Value> = [
            ("severity".into(), serde_yaml::Value::String("high".into())),
            ("security".into(), serde_yaml::Value::Bool(true)),
        ]
        .into_iter()
        .collect();
        [("classifier".to_string(), fm)].into_iter().collect()
    }

    #[test]
    fn merge_spawns_when_suppressed_else_branch_never_runs() {
        let pipeline = fanout_merge_pipeline();

        // classifier + the two matched branches completed; backlog never spawned
        // (its `else` edge was suppressed). The second branch (security) is the
        // node we're processing as "just completed".
        let mut state = empty_run_state();
        state
            .nodes
            .insert("classifier".into(), completed_node("classifier"));
        state
            .nodes
            .insert("hotfix".into(), completed_node("hotfix"));
        state
            .nodes
            .insert("security".into(), completed_node("security"));

        let fm_by_node = classifier_high_security_fm();
        let actions = evaluate_outgoing_edges_full(
            &pipeline,
            &state,
            "security",
            &HashMap::new(),
            &HashMap::new(),
            &fm_by_node,
        );

        assert!(
            actions.contains(&SchedulerAction::Spawn {
                node_id: "merge1".into(),
                iter: 1,
            }),
            "merge must spawn once both fired branches completed, ignoring the \
             permanently-suppressed backlog branch: {actions:?}"
        );
    }

    #[test]
    fn merge_still_waits_for_a_fired_branch_that_is_not_yet_done() {
        // The suppression relief must NOT let a Merge fire early: while a branch
        // that DID fire (hotfix) is still running, the Merge must keep waiting.
        let pipeline = fanout_merge_pipeline();

        let mut state = empty_run_state();
        state
            .nodes
            .insert("classifier".into(), completed_node("classifier"));
        state.nodes.insert("hotfix".into(), running_node("hotfix"));
        state
            .nodes
            .insert("security".into(), completed_node("security"));

        let fm_by_node = classifier_high_security_fm();
        let actions = evaluate_outgoing_edges_full(
            &pipeline,
            &state,
            "security",
            &HashMap::new(),
            &HashMap::new(),
            &fm_by_node,
        );

        assert!(
            !actions.contains(&SchedulerAction::Spawn {
                node_id: "merge1".into(),
                iter: 1,
            }),
            "merge must NOT spawn while a fired branch (hotfix) is still running: {actions:?}"
        );
    }

    /// Edge case (c) — non-regression: a classic all-unconditional fan-in still
    /// converges. Two unconditional branches into a Merge, both completed, must
    /// spawn the Merge. (The edge-resolution barrier must not break the simple,
    /// pre-conditional case.)
    fn unconditional_fanin_pipeline() -> PipelineDef {
        PipelineDef {
            name: "uncond-fanin".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("a", &["task"], &["out"]),
                make_node("b", &["task"], &["out"]),
                make_merge_node("merge1"),
                make_end_node(),
            ],
            edges: vec![
                make_edge("a", "out", "merge1", "branches"),
                make_edge("b", "out", "merge1", "branches"),
                make_end_edge("merge1", "merged", "done"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        }
    }

    #[test]
    fn unconditional_fanin_still_converges() {
        let pipeline = unconditional_fanin_pipeline();
        let mut state = empty_run_state();
        state.nodes.insert("a".into(), completed_node("a"));
        state.nodes.insert("b".into(), completed_node("b"));

        let actions = evaluate_outgoing_edges_full(
            &pipeline,
            &state,
            "b",
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );

        assert!(
            actions.contains(&SchedulerAction::Spawn {
                node_id: "merge1".into(),
                iter: 1,
            }),
            "classic unconditional fan-in must still converge on merge1: {actions:?}"
        );
    }

    /// Edge case (d) — death propagation over >=2 levels. `mid` is fed by a
    /// single guarded edge from `classifier` that did not fire (its sibling
    /// guard matched), so `mid` is dead; `merge1` is fed by `mid` (2nd-level
    /// dead branch) and by `hotfix` (live, completed). The Merge must spawn on
    /// the single live branch, treating the transitively-dead `mid` branch as
    /// resolved.
    fn two_level_death_pipeline() -> PipelineDef {
        PipelineDef {
            name: "two-level-death".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("classifier", &["task"], &["triage"]),
                make_node("hotfix", &["triage"], &["patch"]),
                make_node("mid", &["triage"], &["out"]),
                make_merge_node("merge1"),
                make_end_node(),
            ],
            edges: vec![
                // hotfix branch fires (severity=high), mid branch does not.
                make_cond_edge(
                    "classifier",
                    "triage",
                    "hotfix",
                    "triage",
                    Some("severity: { eq: high }"),
                    false,
                ),
                make_cond_edge(
                    "classifier",
                    "triage",
                    "mid",
                    "triage",
                    Some("severity: { eq: low }"),
                    false,
                ),
                make_edge("hotfix", "patch", "merge1", "branches"),
                make_edge("mid", "out", "merge1", "branches"),
                make_end_edge("merge1", "merged", "done"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        }
    }

    #[test]
    fn merge_spawns_past_two_level_dead_branch() {
        let pipeline = two_level_death_pipeline();
        let mut state = empty_run_state();
        state
            .nodes
            .insert("classifier".into(), completed_node("classifier"));
        state
            .nodes
            .insert("hotfix".into(), completed_node("hotfix"));
        // `mid` never spawned: its inbound guarded edge did not fire.

        let fm: HashMap<String, serde_yaml::Value> =
            [("severity".into(), serde_yaml::Value::String("high".into()))]
                .into_iter()
                .collect();
        let fm_by_node: HashMap<String, HashMap<String, serde_yaml::Value>> =
            [("classifier".to_string(), fm)].into_iter().collect();

        let actions = evaluate_outgoing_edges_full(
            &pipeline,
            &state,
            "hotfix",
            &HashMap::new(),
            &HashMap::new(),
            &fm_by_node,
        );

        assert!(
            actions.contains(&SchedulerAction::Spawn {
                node_id: "merge1".into(),
                iter: 1,
            }),
            "merge must spawn past a transitively-dead (2-level) branch: {actions:?}"
        );
    }

    /// Edge case (a) — an all-dead Merge is SKIPPED when End stays reachable.
    /// Both branches into `merge1` are guarded and neither matched, so `merge1`
    /// has zero fired branches and is itself dead. A separate unconditional path
    /// `classifier -> end` keeps End reachable, so the run must reach End rather
    /// than stall waiting on the dead `merge1`.
    fn all_dead_merge_with_alt_end_pipeline() -> PipelineDef {
        PipelineDef {
            name: "all-dead-merge".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("classifier", &["task"], &["triage"]),
                make_node("hotfix", &["triage"], &["patch"]),
                make_node("security", &["triage"], &["review"]),
                make_merge_node("merge1"),
                make_end_node(),
            ],
            edges: vec![
                make_cond_edge(
                    "classifier",
                    "triage",
                    "hotfix",
                    "triage",
                    Some("severity: { eq: high }"),
                    false,
                ),
                make_cond_edge(
                    "classifier",
                    "triage",
                    "security",
                    "triage",
                    Some("security: { eq: true }"),
                    false,
                ),
                make_edge("hotfix", "patch", "merge1", "branches"),
                make_edge("security", "review", "merge1", "branches"),
                // merge1 -> end, AND a direct classifier -> end keeping End reachable.
                make_end_edge("merge1", "merged", "merged-done"),
                make_end_edge("classifier", "triage", "direct-done"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        }
    }

    #[test]
    fn all_dead_merge_is_skipped_when_end_reachable() {
        let pipeline = all_dead_merge_with_alt_end_pipeline();
        let mut state = empty_run_state();
        state
            .nodes
            .insert("classifier".into(), completed_node("classifier"));

        // Artifact matches NEITHER guard: both hotfix and security branches die,
        // so merge1 has zero fired branches.
        let fm: HashMap<String, serde_yaml::Value> =
            [("severity".into(), serde_yaml::Value::String("low".into()))]
                .into_iter()
                .collect();
        let fm_by_node: HashMap<String, HashMap<String, serde_yaml::Value>> =
            [("classifier".to_string(), fm.clone())]
                .into_iter()
                .collect();

        let actions = evaluate_outgoing_edges_full(
            &pipeline,
            &state,
            "classifier",
            &HashMap::new(),
            &fm,
            &fm_by_node,
        );

        // The direct edge fires End; the run must not stall on the dead merge1.
        assert!(
            actions.contains(&SchedulerAction::Complete)
                || actions
                    .iter()
                    .any(|a| matches!(a, SchedulerAction::Halt { .. })),
            "an all-dead merge must not silently stall the run: {actions:?}"
        );
        assert!(
            !actions.contains(&SchedulerAction::Spawn {
                node_id: "merge1".into(),
                iter: 1,
            }),
            "an all-dead merge must NOT spawn: {actions:?}"
        );
    }

    /// Edge case (b) — death cascade reaches End: explicit halt, never a silent
    /// stall. The ONLY path to End is via `merge1`; both branches into `merge1`
    /// are guarded and neither matched, so `merge1` is all-dead and End becomes
    /// unreachable. Per ADR-0011 ("jamais de stall silencieux") the scheduler
    /// must emit an explicit Halt rather than leaving the run Running forever.
    fn all_dead_merge_only_end_pipeline() -> PipelineDef {
        PipelineDef {
            name: "all-dead-only-end".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("classifier", &["task"], &["triage"]),
                make_node("hotfix", &["triage"], &["patch"]),
                make_node("security", &["triage"], &["review"]),
                make_merge_node("merge1"),
                make_end_node(),
            ],
            edges: vec![
                make_cond_edge(
                    "classifier",
                    "triage",
                    "hotfix",
                    "triage",
                    Some("severity: { eq: high }"),
                    false,
                ),
                make_cond_edge(
                    "classifier",
                    "triage",
                    "security",
                    "triage",
                    Some("security: { eq: true }"),
                    false,
                ),
                make_edge("hotfix", "patch", "merge1", "branches"),
                make_edge("security", "review", "merge1", "branches"),
                make_end_edge("merge1", "merged", "done"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        }
    }

    #[test]
    fn death_cascade_to_unreachable_end_halts_explicitly() {
        let pipeline = all_dead_merge_only_end_pipeline();
        let mut state = empty_run_state();
        state
            .nodes
            .insert("classifier".into(), completed_node("classifier"));

        // Artifact matches neither guard: both branches die, merge1 is all-dead,
        // and End (reachable only through merge1) becomes unreachable.
        let fm: HashMap<String, serde_yaml::Value> =
            [("severity".into(), serde_yaml::Value::String("low".into()))]
                .into_iter()
                .collect();
        let fm_by_node: HashMap<String, HashMap<String, serde_yaml::Value>> =
            [("classifier".to_string(), fm.clone())]
                .into_iter()
                .collect();

        let actions = evaluate_outgoing_edges_full(
            &pipeline,
            &state,
            "classifier",
            &HashMap::new(),
            &fm,
            &fm_by_node,
        );

        assert!(
            actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::Halt { .. })),
            "a death cascade rendering End unreachable must halt explicitly, \
             never stall silently: {actions:?}"
        );
    }

    /// Guard against a false-positive halt: while a branch that DID fire is
    /// still running, End is still reachable through it, so the unrouted-halt
    /// detector must stay its hand. The Merge keeps waiting; no Halt is emitted.
    #[test]
    fn no_halt_while_a_fired_branch_is_still_running() {
        // Same shape as all_dead_merge_only_end, but the artifact matches a guard
        // (severity=high), so `hotfix` fired and is running; `security` died.
        let pipeline = all_dead_merge_only_end_pipeline();
        let mut state = empty_run_state();
        state
            .nodes
            .insert("classifier".into(), completed_node("classifier"));
        state.nodes.insert("hotfix".into(), running_node("hotfix"));

        let fm: HashMap<String, serde_yaml::Value> =
            [("severity".into(), serde_yaml::Value::String("high".into()))]
                .into_iter()
                .collect();
        let fm_by_node: HashMap<String, HashMap<String, serde_yaml::Value>> =
            [("classifier".to_string(), fm.clone())]
                .into_iter()
                .collect();

        // Re-evaluate the classifier (e.g. on a later tick): hotfix already
        // spawned (running), security dead. End reachable through hotfix->merge1.
        let actions = evaluate_outgoing_edges_full(
            &pipeline,
            &state,
            "classifier",
            &HashMap::new(),
            &fm,
            &fm_by_node,
        );

        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::Halt { .. })),
            "must NOT halt while a fired branch (hotfix) is still running and \
             End stays reachable through it: {actions:?}"
        );
    }

    #[test]
    fn end_edge_always_fires() {
        let pipeline = PipelineDef {
            name: "halt-test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("reviewer", &["code"], &["review"]),
                make_end_node(),
            ],
            edges: vec![make_end_edge("reviewer", "review", "Run halted")],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("reviewer".into(), completed_node("reviewer"));

        let actions = evaluate_outgoing_edges(&pipeline, &state, "reviewer");
        assert_eq!(
            actions,
            vec![SchedulerAction::Halt {
                message: "Run halted".into(),
            }]
        );
    }

    // --- Switch node tests ---

    fn make_switch_node(id: &str, branch_outputs: Vec<Port>) -> NodeDef {
        NodeDef {
            id: id.into(),
            name: id.into(),
            node_type: NodeType::Switch,
            inputs: vec![Port {
                name: "in".into(),
                repeated: false,
                side: None,
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
            }],
            outputs: branch_outputs,
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
        }
    }

    fn switch_port(name: &str, when_yaml: &str) -> Port {
        Port {
            name: name.into(),
            repeated: false,
            side: None,
            port_type: PortType::Markdown,
            frontmatter: None,
            when: Some(serde_yaml::from_str(when_yaml).unwrap()),
            description: None,
        }
    }

    fn switch_default_port() -> Port {
        Port {
            name: "default".into(),
            repeated: false,
            side: None,
            port_type: PortType::Markdown,
            frontmatter: None,
            when: None,
            description: None,
        }
    }

    #[test]
    fn switch_routes_to_matched_branch_only() {
        let pipeline = PipelineDef {
            name: "switch-test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_switch_node(
                    "sw",
                    vec![
                        switch_port("pass", "verdict: { in: [PASS, APPROVED] }"),
                        switch_default_port(),
                    ],
                ),
                make_node("b-pass", &["in"], &["out"]),
                make_node("c-default", &["in"], &["out"]),
            ],
            edges: vec![
                make_edge("sw", "pass", "b-pass", "in"),
                make_edge("sw", "default", "c-default", "in"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state.nodes.insert("sw".into(), completed_node("sw"));

        let mut fm = HashMap::new();
        fm.insert("verdict".into(), serde_yaml::Value::String("PASS".into()));

        let actions =
            evaluate_outgoing_edges_with_context(&pipeline, &state, "sw", &HashMap::new(), &fm);

        assert!(actions.contains(&SchedulerAction::SwitchRouted {
            node_id: "sw".into(),
            chosen_branch: "pass".into(),
        }));
        assert!(actions.contains(&SchedulerAction::Spawn {
            node_id: "b-pass".into(),
            iter: 1,
        }));
        assert!(!actions.iter().any(|a| matches!(a,
            SchedulerAction::Spawn { node_id, .. } if node_id == "c-default"
        )));
    }

    #[test]
    fn switch_falls_through_to_default() {
        let pipeline = PipelineDef {
            name: "switch-default".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_switch_node(
                    "sw",
                    vec![
                        switch_port("pass", "verdict: { eq: PASS }"),
                        switch_default_port(),
                    ],
                ),
                make_node("b-pass", &["in"], &["out"]),
                make_node("c-default", &["in"], &["out"]),
            ],
            edges: vec![
                make_edge("sw", "pass", "b-pass", "in"),
                make_edge("sw", "default", "c-default", "in"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state.nodes.insert("sw".into(), completed_node("sw"));

        let fm: HashMap<String, serde_yaml::Value> =
            [("verdict".into(), serde_yaml::Value::String("FAIL".into()))]
                .into_iter()
                .collect();

        let actions =
            evaluate_outgoing_edges_with_context(&pipeline, &state, "sw", &HashMap::new(), &fm);

        assert!(actions.contains(&SchedulerAction::SwitchRouted {
            node_id: "sw".into(),
            chosen_branch: "default".into(),
        }));
        assert!(actions.contains(&SchedulerAction::Spawn {
            node_id: "c-default".into(),
            iter: 1,
        }));
        assert!(!actions.iter().any(|a| matches!(a,
            SchedulerAction::Spawn { node_id, .. } if node_id == "b-pass"
        )));
    }

    #[test]
    fn switch_routed_event_is_emitted() {
        let pipeline = PipelineDef {
            name: "switch-event".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_switch_node(
                    "sw",
                    vec![
                        switch_port("pass", "verdict: { eq: PASS }"),
                        switch_default_port(),
                    ],
                ),
                make_node("downstream", &["in"], &["out"]),
            ],
            edges: vec![make_edge("sw", "pass", "downstream", "in")],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state.nodes.insert("sw".into(), completed_node("sw"));

        let fm: HashMap<String, serde_yaml::Value> =
            [("verdict".into(), serde_yaml::Value::String("PASS".into()))]
                .into_iter()
                .collect();

        let actions =
            evaluate_outgoing_edges_with_context(&pipeline, &state, "sw", &HashMap::new(), &fm);

        let switch_routed_actions: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, SchedulerAction::SwitchRouted { .. }))
            .collect();
        assert_eq!(switch_routed_actions.len(), 1);
        assert_eq!(
            switch_routed_actions[0],
            &SchedulerAction::SwitchRouted {
                node_id: "sw".into(),
                chosen_branch: "pass".into(),
            }
        );
    }

    // --- Inline Switch evaluation (issue #118) ---

    #[test]
    fn upstream_completion_evaluates_switch_inline() {
        // upstream → sw → downstream
        // When upstream completes, the scheduler should evaluate the Switch
        // inline and spawn downstream directly — no Spawn for "sw".
        let pipeline = PipelineDef {
            name: "inline-switch".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("upstream", &["task"], &["out"]),
                make_switch_node(
                    "sw",
                    vec![
                        switch_port("pass", "verdict: { eq: PASS }"),
                        switch_default_port(),
                    ],
                ),
                make_node("pass-handler", &["in"], &["out"]),
                make_node("default-handler", &["in"], &["out"]),
            ],
            edges: vec![
                make_edge("upstream", "out", "sw", "in"),
                make_edge("sw", "pass", "pass-handler", "in"),
                make_edge("sw", "default", "default-handler", "in"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("upstream".into(), completed_node("upstream"));

        let fm: HashMap<String, serde_yaml::Value> =
            [("verdict".into(), serde_yaml::Value::String("PASS".into()))]
                .into_iter()
                .collect();

        let actions = evaluate_outgoing_edges_with_context(
            &pipeline,
            &state,
            "upstream",
            &HashMap::new(),
            &fm,
        );

        // Switch should be evaluated inline: SwitchRouted emitted
        assert!(
            actions.contains(&SchedulerAction::SwitchRouted {
                node_id: "sw".into(),
                chosen_branch: "pass".into(),
            }),
            "expected SwitchRouted, got {actions:?}"
        );
        // Downstream of the matched branch should be spawned
        assert!(
            actions.contains(&SchedulerAction::Spawn {
                node_id: "pass-handler".into(),
                iter: 1,
            }),
            "expected Spawn pass-handler, got {actions:?}"
        );
        // No Spawn for the Switch node itself
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::Spawn { node_id, .. } if node_id == "sw")),
            "Switch must NOT be spawned, got {actions:?}"
        );
        // Non-matched branch must NOT be spawned
        assert!(
            !actions.iter().any(
                |a| matches!(a, SchedulerAction::Spawn { node_id, .. } if node_id == "default-handler")
            ),
            "default-handler must NOT be spawned, got {actions:?}"
        );
    }

    #[test]
    fn inline_switch_default_fallthrough() {
        let pipeline = PipelineDef {
            name: "inline-switch-default".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("upstream", &["task"], &["out"]),
                make_switch_node(
                    "sw",
                    vec![
                        switch_port("pass", "verdict: { eq: PASS }"),
                        switch_default_port(),
                    ],
                ),
                make_node("pass-handler", &["in"], &["out"]),
                make_node("default-handler", &["in"], &["out"]),
            ],
            edges: vec![
                make_edge("upstream", "out", "sw", "in"),
                make_edge("sw", "pass", "pass-handler", "in"),
                make_edge("sw", "default", "default-handler", "in"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("upstream".into(), completed_node("upstream"));

        let fm: HashMap<String, serde_yaml::Value> =
            [("verdict".into(), serde_yaml::Value::String("FAIL".into()))]
                .into_iter()
                .collect();

        let actions = evaluate_outgoing_edges_with_context(
            &pipeline,
            &state,
            "upstream",
            &HashMap::new(),
            &fm,
        );

        assert!(actions.contains(&SchedulerAction::SwitchRouted {
            node_id: "sw".into(),
            chosen_branch: "default".into(),
        }));
        assert!(actions.contains(&SchedulerAction::Spawn {
            node_id: "default-handler".into(),
            iter: 1,
        }));
        assert!(!actions.iter().any(
            |a| matches!(a, SchedulerAction::Spawn { node_id, .. } if node_id == "pass-handler")
        ),);
    }

    #[test]
    fn inline_switch_to_end_produces_complete() {
        // upstream → sw → end (via pass branch)
        let pipeline = PipelineDef {
            name: "inline-switch-end".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("upstream", &["task"], &["out"]),
                make_switch_node(
                    "sw",
                    vec![
                        switch_port("pass", "verdict: { eq: PASS }"),
                        switch_default_port(),
                    ],
                ),
                make_node("rework", &["in"], &["out"]),
                make_end_node(),
            ],
            edges: vec![
                make_edge("upstream", "out", "sw", "in"),
                make_edge("sw", "pass", "end", "result"),
                make_edge("sw", "default", "rework", "in"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("upstream".into(), completed_node("upstream"));

        let fm: HashMap<String, serde_yaml::Value> =
            [("verdict".into(), serde_yaml::Value::String("PASS".into()))]
                .into_iter()
                .collect();

        let actions = evaluate_outgoing_edges_with_context(
            &pipeline,
            &state,
            "upstream",
            &HashMap::new(),
            &fm,
        );

        assert!(actions.contains(&SchedulerAction::SwitchRouted {
            node_id: "sw".into(),
            chosen_branch: "pass".into(),
        }));
        assert!(actions.contains(&SchedulerAction::Complete));
    }

    #[test]
    fn inline_switch_to_loop_fires_loop_iter() {
        // upstream → sw(pass) → loop.break
        let pipeline = PipelineDef {
            name: "inline-switch-to-loop".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("upstream", &["task"], &["out"]),
                make_switch_node(
                    "sw",
                    vec![
                        switch_port("pass", "verdict: { eq: PASS }"),
                        switch_default_port(),
                    ],
                ),
                make_loop_node("loop1", 5),
                make_node("rework", &["in"], &["out"]),
            ],
            edges: vec![
                make_edge("upstream", "out", "sw", "in"),
                make_edge("sw", "pass", "loop1", "break"),
                make_edge("sw", "default", "rework", "in"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("upstream".into(), completed_node("upstream"));
        state.loop_states.insert(
            "loop1".into(),
            crate::event_log::LoopState {
                loop_node_id: "loop1".into(),
                current_iter: 1,
                max_iter: 5,
                break_received: false,
                done: false,
            },
        );

        let fm: HashMap<String, serde_yaml::Value> =
            [("verdict".into(), serde_yaml::Value::String("PASS".into()))]
                .into_iter()
                .collect();

        let actions = evaluate_outgoing_edges_with_context(
            &pipeline,
            &state,
            "upstream",
            &HashMap::new(),
            &fm,
        );

        assert!(actions.contains(&SchedulerAction::SwitchRouted {
            node_id: "sw".into(),
            chosen_branch: "pass".into(),
        }));
        assert!(actions.contains(&SchedulerAction::LoopBreakReceived {
            loop_node_id: "loop1".into(),
        }));
    }

    #[test]
    fn inline_switch_first_match_wins_ordering() {
        let pipeline = PipelineDef {
            name: "first-match".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("upstream", &["task"], &["out"]),
                make_switch_node(
                    "sw",
                    vec![
                        switch_port("first", "verdict: { eq: PASS }"),
                        switch_port("second", "verdict: { eq: PASS }"),
                        switch_default_port(),
                    ],
                ),
                make_node("first-handler", &["in"], &["out"]),
                make_node("second-handler", &["in"], &["out"]),
            ],
            edges: vec![
                make_edge("upstream", "out", "sw", "in"),
                make_edge("sw", "first", "first-handler", "in"),
                make_edge("sw", "second", "second-handler", "in"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("upstream".into(), completed_node("upstream"));

        let fm: HashMap<String, serde_yaml::Value> =
            [("verdict".into(), serde_yaml::Value::String("PASS".into()))]
                .into_iter()
                .collect();

        let actions = evaluate_outgoing_edges_with_context(
            &pipeline,
            &state,
            "upstream",
            &HashMap::new(),
            &fm,
        );

        assert!(actions.contains(&SchedulerAction::SwitchRouted {
            node_id: "sw".into(),
            chosen_branch: "first".into(),
        }));
        assert!(actions.contains(&SchedulerAction::Spawn {
            node_id: "first-handler".into(),
            iter: 1,
        }));
        assert!(!actions.iter().any(
            |a| matches!(a, SchedulerAction::Spawn { node_id, .. } if node_id == "second-handler")
        ),);
    }

    #[test]
    fn inline_switch_with_variable_resolution() {
        let pipeline = PipelineDef {
            name: "var-switch".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("upstream", &["task"], &["out"]),
                make_switch_node(
                    "sw",
                    vec![
                        switch_port("high", "score: { gte: \"$threshold\" }"),
                        switch_default_port(),
                    ],
                ),
                make_node("high-handler", &["in"], &["out"]),
                make_node("default-handler", &["in"], &["out"]),
            ],
            edges: vec![
                make_edge("upstream", "out", "sw", "in"),
                make_edge("sw", "high", "high-handler", "in"),
                make_edge("sw", "default", "default-handler", "in"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("upstream".into(), completed_node("upstream"));

        let fm: HashMap<String, serde_yaml::Value> = [(
            "score".into(),
            serde_yaml::Value::Number(serde_yaml::Number::from(8)),
        )]
        .into_iter()
        .collect();
        let vars: HashMap<String, serde_yaml::Value> = [(
            "threshold".into(),
            serde_yaml::Value::Number(serde_yaml::Number::from(7)),
        )]
        .into_iter()
        .collect();

        let actions =
            evaluate_outgoing_edges_with_context(&pipeline, &state, "upstream", &vars, &fm);

        assert!(actions.contains(&SchedulerAction::SwitchRouted {
            node_id: "sw".into(),
            chosen_branch: "high".into(),
        }));
        assert!(actions.contains(&SchedulerAction::Spawn {
            node_id: "high-handler".into(),
            iter: 1,
        }));
    }

    #[test]
    fn inline_switch_waits_for_all_upstream() {
        // Two nodes feed the Switch. Only one is complete — Switch must NOT evaluate yet.
        let pipeline = PipelineDef {
            name: "fan-in-switch".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("a", &["task"], &["out"]),
                make_node("b", &["task"], &["out"]),
                make_switch_node(
                    "sw",
                    vec![
                        switch_port("pass", "verdict: { eq: PASS }"),
                        switch_default_port(),
                    ],
                ),
                make_node("downstream", &["in"], &["out"]),
            ],
            edges: vec![
                make_edge("a", "out", "sw", "in"),
                make_edge("b", "out", "sw", "in"),
                make_edge("sw", "pass", "downstream", "in"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state.nodes.insert("a".into(), completed_node("a"));
        // b is still running
        state.nodes.insert("b".into(), running_node("b"));

        let fm: HashMap<String, serde_yaml::Value> =
            [("verdict".into(), serde_yaml::Value::String("PASS".into()))]
                .into_iter()
                .collect();

        let actions =
            evaluate_outgoing_edges_with_context(&pipeline, &state, "a", &HashMap::new(), &fm);

        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::SwitchRouted { .. })),
            "Switch must not evaluate until all upstream complete, got {actions:?}"
        );
    }

    #[test]
    fn inline_switch_mid_run_clause_edit_changes_routing() {
        let make_pipeline_with_clause = |clause: &str| PipelineDef {
            name: "mid-run-edit".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("upstream", &["task"], &["out"]),
                make_switch_node(
                    "sw",
                    vec![switch_port("pass", clause), switch_default_port()],
                ),
                make_node("pass-handler", &["in"], &["out"]),
                make_node("default-handler", &["in"], &["out"]),
            ],
            edges: vec![
                make_edge("upstream", "out", "sw", "in"),
                make_edge("sw", "pass", "pass-handler", "in"),
                make_edge("sw", "default", "default-handler", "in"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("upstream".into(), completed_node("upstream"));

        let fm: HashMap<String, serde_yaml::Value> =
            [("verdict".into(), serde_yaml::Value::String("PASS".into()))]
                .into_iter()
                .collect();

        // First evaluation: clause matches → routes to "pass"
        let pipeline_v1 = make_pipeline_with_clause("verdict: { eq: PASS }");
        let actions_v1 = evaluate_outgoing_edges_with_context(
            &pipeline_v1,
            &state,
            "upstream",
            &HashMap::new(),
            &fm,
        );
        assert!(
            actions_v1.contains(&SchedulerAction::SwitchRouted {
                node_id: "sw".into(),
                chosen_branch: "pass".into(),
            }),
            "v1 should route to pass"
        );

        // Mid-run edit: change the clause so it no longer matches → routes to "default"
        let pipeline_v2 = make_pipeline_with_clause("verdict: { eq: APPROVED }");
        let actions_v2 = evaluate_outgoing_edges_with_context(
            &pipeline_v2,
            &state,
            "upstream",
            &HashMap::new(),
            &fm,
        );
        assert!(
            actions_v2.contains(&SchedulerAction::SwitchRouted {
                node_id: "sw".into(),
                chosen_branch: "default".into(),
            }),
            "v2 (edited clause) should route to default"
        );
    }

    // --- Loop node tests ---

    fn make_loop_node(id: &str, max_iter: i64) -> NodeDef {
        NodeDef {
            id: id.into(),
            name: id.into(),
            node_type: NodeType::Loop,
            inputs: vec![
                Port {
                    name: "in".into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                },
                Port {
                    name: "break".into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                },
            ],
            outputs: vec![
                Port {
                    name: "body".into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                },
                Port {
                    name: "done".into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                },
            ],
            interactive: false,
            view: None,
            max_iter: Some(serde_yaml::Value::Number(serde_yaml::Number::from(
                max_iter,
            ))),
            over: None,
        }
    }

    #[test]
    fn loop_node_skipped_in_ready_nodes() {
        // Loop nodes are never listed as ready — they are control-flow constructs.
        // Body nodes downstream of a Loop are also not ready (they wait for Loop to fire).
        let pipeline = PipelineDef {
            name: "loop-test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_loop_node("loop1", 5),
                make_node("worker", &["in"], &["out"]),
                make_node("entry", &["task"], &["out"]),
            ],
            edges: vec![
                make_edge("entry", "out", "loop1", "in"),
                make_edge("loop1", "body", "worker", "in"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let state = empty_run_state();
        let ready = ready_nodes(&pipeline, &state);
        // Only entry is ready; loop1 is skipped, worker waits for loop1
        assert_eq!(ready, vec!["entry"]);
    }

    #[test]
    fn edge_to_loop_in_fires_body() {
        let pipeline = PipelineDef {
            name: "loop-in".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("upstream", &["task"], &["out"]),
                make_loop_node("loop1", 5),
                make_node("impl", &["in"], &["out"]),
            ],
            edges: vec![
                make_edge("upstream", "out", "loop1", "in"),
                make_edge("loop1", "body", "impl", "in"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("upstream".into(), completed_node("upstream"));

        let actions = evaluate_outgoing_edges(&pipeline, &state, "upstream");

        assert!(actions.contains(&SchedulerAction::LoopIterStarted {
            loop_node_id: "loop1".into(),
            iter: 1,
            max_iter: 5,
        }));
        assert!(actions.contains(&SchedulerAction::Spawn {
            node_id: "impl".into(),
            iter: 1,
        }));
    }

    #[test]
    fn edge_to_loop_break_emits_break_received() {
        let pipeline = PipelineDef {
            name: "loop-break".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_loop_node("loop1", 5),
                make_node("impl", &["in"], &["out"]),
                make_node("sw", &["in"], &["pass"]),
            ],
            edges: vec![
                make_edge("loop1", "body", "impl", "in"),
                make_edge("impl", "out", "sw", "in"),
                make_edge("sw", "pass", "loop1", "break"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state.nodes.insert("sw".into(), completed_node("sw"));

        let actions = evaluate_outgoing_edges(&pipeline, &state, "sw");

        assert!(actions.contains(&SchedulerAction::LoopBreakReceived {
            loop_node_id: "loop1".into(),
        }));
    }

    #[test]
    fn loop_body_completion_advances_iter() {
        // Loop.body → impl → sw → Loop.break
        // Iter 1 body done, no break, iter < max → advance to iter 2
        let pipeline = PipelineDef {
            name: "loop-advance".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_loop_node("loop1", 5),
                make_node("impl", &["in"], &["out"]),
                make_node("sw", &["in"], &["pass", "default"]),
            ],
            edges: vec![
                make_edge("loop1", "body", "impl", "in"),
                make_edge("impl", "out", "sw", "in"),
                make_edge("sw", "pass", "loop1", "break"),
                make_edge("sw", "default", "impl", "in"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state.loop_states.insert(
            "loop1".into(),
            crate::event_log::LoopState {
                loop_node_id: "loop1".into(),
                current_iter: 1,
                max_iter: 5,
                break_received: false,
                done: false,
            },
        );
        state
            .nodes
            .insert("impl".into(), completed_node_iter("impl", 1));
        state
            .nodes
            .insert("sw".into(), completed_node_iter("sw", 1));

        let actions = evaluate_loop_body_completion(&pipeline, &state, "loop1", &HashMap::new());

        assert!(actions.contains(&SchedulerAction::LoopIterStarted {
            loop_node_id: "loop1".into(),
            iter: 2,
            max_iter: 5,
        }));
        assert!(actions.contains(&SchedulerAction::Spawn {
            node_id: "impl".into(),
            iter: 2,
        }));
        assert!(!actions
            .iter()
            .any(|a| matches!(a, SchedulerAction::LoopDone { .. })));
    }

    #[test]
    fn loop_body_completion_with_break_fires_done() {
        let pipeline = PipelineDef {
            name: "loop-break-done".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_loop_node("loop1", 5),
                make_node("impl", &["in"], &["out"]),
                make_node("sw", &["in"], &["pass", "default"]),
                make_end_node(),
            ],
            edges: vec![
                make_edge("loop1", "body", "impl", "in"),
                make_edge("impl", "out", "sw", "in"),
                make_edge("sw", "pass", "loop1", "break"),
                make_edge("loop1", "done", "end", "result"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state.loop_states.insert(
            "loop1".into(),
            crate::event_log::LoopState {
                loop_node_id: "loop1".into(),
                current_iter: 3,
                max_iter: 5,
                break_received: true,
                done: false,
            },
        );
        state
            .nodes
            .insert("impl".into(), completed_node_iter("impl", 3));
        state
            .nodes
            .insert("sw".into(), completed_node_iter("sw", 3));

        let actions = evaluate_loop_body_completion(&pipeline, &state, "loop1", &HashMap::new());

        assert!(actions.contains(&SchedulerAction::LoopDone {
            loop_node_id: "loop1".into(),
        }));
        assert!(actions.contains(&SchedulerAction::Complete));
        assert!(!actions
            .iter()
            .any(|a| matches!(a, SchedulerAction::LoopMaxReached { .. })));
    }

    #[test]
    fn loop_max_iter_reached_fires_done() {
        let pipeline = PipelineDef {
            name: "loop-max".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_loop_node("loop1", 3),
                make_node("impl", &["in"], &["out"]),
                make_node("downstream", &["in"], &["out"]),
            ],
            edges: vec![
                make_edge("loop1", "body", "impl", "in"),
                make_edge("impl", "out", "loop1", "break"),
                make_edge("loop1", "done", "downstream", "in"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state.loop_states.insert(
            "loop1".into(),
            crate::event_log::LoopState {
                loop_node_id: "loop1".into(),
                current_iter: 3,
                max_iter: 3,
                break_received: false,
                done: false,
            },
        );
        state
            .nodes
            .insert("impl".into(), completed_node_iter("impl", 3));

        let actions = evaluate_loop_body_completion(&pipeline, &state, "loop1", &HashMap::new());

        assert!(actions.contains(&SchedulerAction::LoopMaxReached {
            loop_node_id: "loop1".into(),
            max_iter: 3,
        }));
        assert!(actions.contains(&SchedulerAction::LoopDone {
            loop_node_id: "loop1".into(),
        }));
        assert!(actions.contains(&SchedulerAction::Spawn {
            node_id: "downstream".into(),
            iter: 1,
        }));
    }

    #[test]
    fn body_to_break_edge_stops_loop_at_iter_1_when_state_is_refreshed() {
        // Loop.body → impl → Loop.break (no switch — every body completion
        // unconditionally fires break). The orchestration in
        // lib.rs::handle_node_completion runs two passes against the same
        // RunState. If pass 2 sees the LoopBreakReceived just emitted by
        // pass 1, the loop must terminate at iter 1.
        //
        // Regression: before the reload_run_state fix in lib.rs, pass 2 ran
        // against a stale snapshot where break_received=false and wrongly
        // advanced to iter 2. This test pins down the contract: when the
        // dispatcher correctly re-projects between passes, evaluate_loop_body_completion
        // sees break_received=true and emits LoopDone (not LoopIterStarted{2}).
        let pipeline = PipelineDef {
            name: "body-to-break".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_loop_node("loop1", 3),
                make_node("impl", &["in"], &["out"]),
                make_end_node(),
            ],
            edges: vec![
                make_edge("loop1", "body", "impl", "in"),
                make_edge("impl", "out", "loop1", "break"),
                make_edge("loop1", "done", "end", "result"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state.loop_states.insert(
            "loop1".into(),
            crate::event_log::LoopState {
                loop_node_id: "loop1".into(),
                current_iter: 1,
                max_iter: 3,
                break_received: false,
                done: false,
            },
        );
        state
            .nodes
            .insert("impl".into(), completed_node_iter("impl", 1));

        // Pass 1: outgoing edges of impl emit LoopBreakReceived.
        let pass1 = evaluate_outgoing_edges(&pipeline, &state, "impl");
        assert!(
            pass1.contains(&SchedulerAction::LoopBreakReceived {
                loop_node_id: "loop1".into(),
            }),
            "expected LoopBreakReceived in pass 1, got {pass1:?}"
        );

        // Mirror the projection of LoopBreakReceived (event_log.rs:395-403).
        // In production, lib.rs::handle_node_completion achieves the same by
        // calling reload_run_state between passes.
        for action in &pass1 {
            if let SchedulerAction::LoopBreakReceived { loop_node_id } = action {
                if let Some(ls) = state.loop_states.get_mut(loop_node_id) {
                    ls.break_received = true;
                }
            }
        }

        // Pass 2: body completion check with refreshed state.
        let pass2 = evaluate_loop_body_completion(&pipeline, &state, "loop1", &HashMap::new());
        assert!(
            pass2.contains(&SchedulerAction::LoopDone {
                loop_node_id: "loop1".into(),
            }),
            "expected LoopDone after break received, got {pass2:?}"
        );
        assert!(
            !pass2
                .iter()
                .any(|a| matches!(a, SchedulerAction::LoopIterStarted { iter: 2, .. })),
            "must NOT advance to iter 2 once break_received=true, got {pass2:?}"
        );
    }

    #[test]
    fn body_to_break_with_stale_state_wrongly_advances_iter() {
        // This pins down the *bug shape* the reload_run_state fix prevents.
        // If the dispatcher fails to refresh the RunState between passes,
        // evaluate_loop_body_completion still observes break_received=false
        // and emits LoopIterStarted{iter=2}. Catching this in CI ensures any
        // future regression of the orchestration contract is loud.
        let pipeline = PipelineDef {
            name: "body-to-break-stale".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_loop_node("loop1", 3),
                make_node("impl", &["in"], &["out"]),
                make_end_node(),
            ],
            edges: vec![
                make_edge("loop1", "body", "impl", "in"),
                make_edge("impl", "out", "loop1", "break"),
                make_edge("loop1", "done", "end", "result"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state.loop_states.insert(
            "loop1".into(),
            crate::event_log::LoopState {
                loop_node_id: "loop1".into(),
                current_iter: 1,
                max_iter: 3,
                break_received: false,
                done: false,
            },
        );
        state
            .nodes
            .insert("impl".into(), completed_node_iter("impl", 1));

        // Pass 1 emits LoopBreakReceived (intentionally NOT applied to state).
        let _pass1 = evaluate_outgoing_edges(&pipeline, &state, "impl");

        // Pass 2 against the same stale state — this is the buggy path.
        let pass2 = evaluate_loop_body_completion(&pipeline, &state, "loop1", &HashMap::new());
        assert!(
            pass2
                .iter()
                .any(|a| matches!(a, SchedulerAction::LoopIterStarted { iter: 2, .. })),
            "stale state must produce the bug — i.e. iter 2 spawn — to keep \
             reload_run_state honest. Got {pass2:?}"
        );
    }

    #[test]
    fn loop_body_not_complete_no_action() {
        let pipeline = PipelineDef {
            name: "loop-partial".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_loop_node("loop1", 5),
                make_node("impl", &["in"], &["out"]),
                make_node("reviewer", &["in"], &["review"]),
            ],
            edges: vec![
                make_edge("loop1", "body", "impl", "in"),
                make_edge("impl", "out", "reviewer", "in"),
                make_edge("reviewer", "review", "loop1", "break"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state.loop_states.insert(
            "loop1".into(),
            crate::event_log::LoopState {
                loop_node_id: "loop1".into(),
                current_iter: 1,
                max_iter: 5,
                break_received: false,
                done: false,
            },
        );
        // impl done but reviewer still running
        state.nodes.insert("impl".into(), completed_node("impl"));
        state
            .nodes
            .insert("reviewer".into(), running_node("reviewer"));

        let actions = evaluate_loop_body_completion(&pipeline, &state, "loop1", &HashMap::new());

        assert!(actions.is_empty());
    }

    #[test]
    fn break_received_fires_done_even_with_incomplete_body() {
        // After node invalidation, body nodes may be missing from run_state.
        // A break must fire done unconditionally — it never waits for body
        // completion.
        let pipeline = PipelineDef {
            name: "loop-break-incomplete".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_loop_node("loop1", 5),
                make_node("impl", &["in"], &["out"]),
                make_node("tester", &["in"], &["out"]),
                make_node("downstream", &["in"], &["out"]),
            ],
            edges: vec![
                make_edge("loop1", "body", "impl", "in"),
                make_edge("impl", "out", "tester", "in"),
                make_edge("tester", "out", "loop1", "break"),
                make_edge("loop1", "done", "downstream", "in"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state.loop_states.insert(
            "loop1".into(),
            crate::event_log::LoopState {
                loop_node_id: "loop1".into(),
                current_iter: 1,
                max_iter: 5,
                break_received: true,
                done: false,
            },
        );
        // impl was invalidated — NOT in run_state.nodes
        // tester completed (it fired the break)
        state
            .nodes
            .insert("tester".into(), completed_node_iter("tester", 1));

        let actions = evaluate_loop_body_completion(&pipeline, &state, "loop1", &HashMap::new());

        assert!(
            actions.contains(&SchedulerAction::LoopDone {
                loop_node_id: "loop1".into(),
            }),
            "break_received must fire LoopDone regardless of body state, got {actions:?}"
        );
        assert!(
            actions.contains(&SchedulerAction::Spawn {
                node_id: "downstream".into(),
                iter: 1,
            }),
            "break_received must fire done port to spawn downstream, got {actions:?}"
        );
    }

    // --- seed_pending_loops tests ---

    fn make_start_node(id: &str) -> NodeDef {
        NodeDef {
            id: id.into(),
            name: id.into(),
            node_type: NodeType::Start,
            inputs: vec![],
            outputs: vec![Port {
                name: "user_prompt".into(),
                repeated: false,
                side: None,
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
            }],
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
        }
    }

    #[test]
    fn seed_pending_loops_emits_iter_started_when_start_feeds_loop() {
        // Start → loop1.in   loop1.body → impl
        // At run start, seed_pending_loops must emit LoopIterStarted{1} +
        // Spawn{impl, 1}, otherwise the run is stuck.
        let pipeline = PipelineDef {
            name: "start-loop".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_start_node("start"),
                make_loop_node("loop1", 5),
                make_node("impl", &["in"], &["out"]),
            ],
            edges: vec![
                make_edge("start", "user_prompt", "loop1", "in"),
                make_edge("loop1", "body", "impl", "in"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };
        let state = empty_run_state();

        let actions = seed_pending_loops(&pipeline, &state, &HashMap::new());

        assert!(actions.contains(&SchedulerAction::LoopIterStarted {
            loop_node_id: "loop1".into(),
            iter: 1,
            max_iter: 5,
        }));
        assert!(actions.contains(&SchedulerAction::Spawn {
            node_id: "impl".into(),
            iter: 1,
        }));
    }

    #[test]
    fn seed_pending_loops_propagates_max_iter_from_loop_node_spec() {
        // Regression: previously LoopIterStarted defaulted to max_iter=5 in
        // loop_states, even when the spec said 3. Now it must reflect the spec.
        let pipeline = PipelineDef {
            name: "max-iter-3".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_start_node("start"),
                make_loop_node("loop1", 3),
                make_node("impl", &["in"], &["out"]),
            ],
            edges: vec![
                make_edge("start", "user_prompt", "loop1", "in"),
                make_edge("loop1", "body", "impl", "in"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };
        let state = empty_run_state();

        let actions = seed_pending_loops(&pipeline, &state, &HashMap::new());

        assert!(actions.contains(&SchedulerAction::LoopIterStarted {
            loop_node_id: "loop1".into(),
            iter: 1,
            max_iter: 3,
        }));
    }

    #[test]
    fn seed_pending_loops_idempotent_after_iter_started() {
        // Once the loop has a loop_state, seed must not re-emit.
        let pipeline = PipelineDef {
            name: "start-loop".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_start_node("start"),
                make_loop_node("loop1", 5),
                make_node("impl", &["in"], &["out"]),
            ],
            edges: vec![
                make_edge("start", "user_prompt", "loop1", "in"),
                make_edge("loop1", "body", "impl", "in"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };
        let mut state = empty_run_state();
        state.loop_states.insert(
            "loop1".into(),
            crate::event_log::LoopState {
                loop_node_id: "loop1".into(),
                current_iter: 1,
                max_iter: 5,
                break_received: false,
                done: false,
            },
        );

        let actions = seed_pending_loops(&pipeline, &state, &HashMap::new());
        assert!(actions.is_empty());
    }

    #[test]
    fn seed_pending_loops_skipped_when_in_edge_missing() {
        // Loop has no edge feeding `in` — cannot bootstrap.
        let pipeline = PipelineDef {
            name: "loop-no-in".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![make_start_node("start"), make_loop_node("loop1", 5)],
            edges: vec![],
            loops: Vec::new(),
            prompt_required: true,
        };
        let state = empty_run_state();

        let actions = seed_pending_loops(&pipeline, &state, &HashMap::new());
        assert!(actions.is_empty());
    }

    #[test]
    fn seed_pending_loops_waits_when_upstream_non_start_not_completed() {
        // upstream(running) → loop1.in. Don't seed yet.
        let pipeline = PipelineDef {
            name: "loop-waiting".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("upstream", &["x"], &["out"]),
                make_loop_node("loop1", 5),
                make_node("impl", &["in"], &["out"]),
            ],
            edges: vec![
                make_edge("upstream", "out", "loop1", "in"),
                make_edge("loop1", "body", "impl", "in"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };
        let mut state = empty_run_state();
        state
            .nodes
            .insert("upstream".into(), running_node("upstream"));

        let actions = seed_pending_loops(&pipeline, &state, &HashMap::new());
        assert!(actions.is_empty());
    }

    #[test]
    fn seed_pending_loops_fires_for_all_body_targets() {
        // loop.body fan-outs to two targets — both should be spawned at iter 1.
        let pipeline = PipelineDef {
            name: "fanout".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_start_node("start"),
                make_loop_node("loop1", 3),
                make_node("a", &["in"], &["out"]),
                make_node("b", &["in"], &["out"]),
            ],
            edges: vec![
                make_edge("start", "user_prompt", "loop1", "in"),
                make_edge("loop1", "body", "a", "in"),
                make_edge("loop1", "body", "b", "in"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };
        let state = empty_run_state();

        let actions = seed_pending_loops(&pipeline, &state, &HashMap::new());

        assert!(actions.contains(&SchedulerAction::Spawn {
            node_id: "a".into(),
            iter: 1,
        }));
        assert!(actions.contains(&SchedulerAction::Spawn {
            node_id: "b".into(),
            iter: 1,
        }));
    }

    // --- ForEach dispatch ---

    fn make_foreach_node_with_over(id: &str, over: &str) -> NodeDef {
        let mut node = make_foreach_node(id);
        node.over = Some(over.into());
        node
    }

    fn make_foreach_node(id: &str) -> NodeDef {
        NodeDef {
            id: id.into(),
            name: id.into(),
            node_type: NodeType::ForEach,
            inputs: vec![
                Port {
                    name: "in".into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                },
                Port {
                    name: "break".into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                },
            ],
            outputs: vec![
                Port {
                    name: "body".into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                },
                Port {
                    name: "done".into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                },
            ],
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
        }
    }

    #[test]
    fn foreach_empty_list_fires_done_immediately() {
        let pipeline = PipelineDef {
            name: "foreach-empty".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("upstream", &["in"], &["out"]),
                make_foreach_node("fe1"),
                make_node("worker", &["in"], &["out"]),
                make_end_node(),
            ],
            edges: vec![
                make_edge("upstream", "out", "fe1", "in"),
                make_edge("fe1", "body", "worker", "in"),
                make_edge("fe1", "done", "end", "result"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("upstream".into(), completed_node("upstream"));

        let frontmatter = HashMap::new();
        let actions = evaluate_outgoing_edges_with_context(
            &pipeline,
            &state,
            "upstream",
            &HashMap::new(),
            &frontmatter,
        );

        assert!(actions.contains(&SchedulerAction::ForEachEmpty {
            foreach_node_id: "fe1".into(),
        }));
        assert!(actions.contains(&SchedulerAction::ForEachDone {
            foreach_node_id: "fe1".into(),
        }));
        assert!(actions.contains(&SchedulerAction::Complete));
        assert!(
            !actions.iter().any(
                |a| matches!(a, SchedulerAction::Spawn { node_id, .. } if node_id == "worker")
            ),
            "empty list should not spawn body nodes"
        );
    }

    #[test]
    fn foreach_list_of_3_spawns_3_body_iterations() {
        let pipeline = PipelineDef {
            name: "foreach-3".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("upstream", &["in"], &["out"]),
                make_foreach_node("fe1"),
                make_node("worker", &["in"], &["out"]),
                make_end_node(),
            ],
            edges: vec![
                make_edge("upstream", "out", "fe1", "in"),
                make_edge("fe1", "body", "worker", "in"),
                make_edge("worker", "out", "fe1", "done"),
                make_edge("fe1", "done", "end", "result"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("upstream".into(), completed_node("upstream"));

        let mut frontmatter = HashMap::new();
        frontmatter.insert(
            "items".into(),
            serde_yaml::Value::Sequence(vec![
                serde_yaml::Value::String("alpha".into()),
                serde_yaml::Value::String("beta".into()),
                serde_yaml::Value::String("gamma".into()),
            ]),
        );

        let actions = evaluate_outgoing_edges_with_context(
            &pipeline,
            &state,
            "upstream",
            &HashMap::new(),
            &frontmatter,
        );

        assert!(actions.contains(&SchedulerAction::ForEachStarted {
            foreach_node_id: "fe1".into(),
            total_items: 3,
            items: vec![
                serde_yaml::Value::String("alpha".into()),
                serde_yaml::Value::String("beta".into()),
                serde_yaml::Value::String("gamma".into()),
            ],
        }));

        for i in 1..=3 {
            assert!(
                actions.contains(&SchedulerAction::Spawn {
                    node_id: "worker".into(),
                    iter: i,
                }),
                "should spawn worker iter {i}"
            );
        }
    }

    #[test]
    fn foreach_break_mid_iteration() {
        let pipeline = PipelineDef {
            name: "foreach-break".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("upstream", &["in"], &["out"]),
                make_foreach_node("fe1"),
                make_node("worker", &["in"], &["out"]),
                make_end_node(),
            ],
            edges: vec![
                make_edge("upstream", "out", "fe1", "in"),
                make_edge("fe1", "body", "worker", "in"),
                make_edge("worker", "out", "fe1", "break"),
                make_edge("fe1", "done", "end", "result"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("upstream".into(), completed_node("upstream"));
        state.foreach_states.insert(
            "fe1".into(),
            crate::event_log::ForEachState {
                foreach_node_id: "fe1".into(),
                total_items: 3,
                break_received: false,
                done: false,
            },
        );
        state
            .nodes
            .insert("worker".into(), completed_node("worker"));

        let actions = evaluate_outgoing_edges_with_context(
            &pipeline,
            &state,
            "worker",
            &HashMap::new(),
            &HashMap::new(),
        );

        assert!(actions.contains(&SchedulerAction::ForEachBreakReceived {
            foreach_node_id: "fe1".into(),
        }));
    }

    #[test]
    fn foreach_body_completion_fires_done() {
        let pipeline = PipelineDef {
            name: "foreach-done".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_foreach_node("fe1"),
                make_node("worker", &["in"], &["out"]),
                make_end_node(),
            ],
            edges: vec![
                make_edge("fe1", "body", "worker", "in"),
                make_edge("worker", "out", "fe1", "done"),
                make_edge("fe1", "done", "end", "result"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state.foreach_states.insert(
            "fe1".into(),
            crate::event_log::ForEachState {
                foreach_node_id: "fe1".into(),
                total_items: 3,
                break_received: false,
                done: false,
            },
        );

        let mut worker_state = completed_node("worker");
        worker_state.iter = 3;
        worker_state.iterations = vec![
            crate::event_log::IterationInfo {
                iter: 1,
                status: NodeStatus::Completed,
                started_at: Some("t0".into()),
                completed_at: Some("t1".into()),
            },
            crate::event_log::IterationInfo {
                iter: 2,
                status: NodeStatus::Completed,
                started_at: Some("t0".into()),
                completed_at: Some("t1".into()),
            },
            crate::event_log::IterationInfo {
                iter: 3,
                status: NodeStatus::Completed,
                started_at: Some("t0".into()),
                completed_at: Some("t1".into()),
            },
        ];
        state.nodes.insert("worker".into(), worker_state);

        let actions = evaluate_foreach_body_completion(&pipeline, &state, "fe1");

        assert!(actions.contains(&SchedulerAction::ForEachDone {
            foreach_node_id: "fe1".into(),
        }));
        assert!(actions.contains(&SchedulerAction::Complete));
    }

    #[test]
    fn foreach_body_not_complete_no_done() {
        let pipeline = PipelineDef {
            name: "foreach-partial".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_foreach_node("fe1"),
                make_node("worker", &["in"], &["out"]),
                make_end_node(),
            ],
            edges: vec![
                make_edge("fe1", "body", "worker", "in"),
                make_edge("worker", "out", "fe1", "done"),
                make_edge("fe1", "done", "end", "result"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state.foreach_states.insert(
            "fe1".into(),
            crate::event_log::ForEachState {
                foreach_node_id: "fe1".into(),
                total_items: 3,
                break_received: false,
                done: false,
            },
        );

        let mut worker_state = completed_node("worker");
        worker_state.iter = 2;
        worker_state.iterations = vec![
            crate::event_log::IterationInfo {
                iter: 1,
                status: NodeStatus::Completed,
                started_at: Some("t0".into()),
                completed_at: Some("t1".into()),
            },
            crate::event_log::IterationInfo {
                iter: 2,
                status: NodeStatus::Completed,
                started_at: Some("t0".into()),
                completed_at: Some("t1".into()),
            },
        ];
        state.nodes.insert("worker".into(), worker_state);

        let actions = evaluate_foreach_body_completion(&pipeline, &state, "fe1");
        assert!(
            actions.is_empty(),
            "should not fire done with only 2 of 3 complete"
        );
    }

    #[test]
    fn foreach_node_skipped_by_ready_nodes() {
        let pipeline = PipelineDef {
            name: "foreach-skip".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_foreach_node("fe1"),
                make_node("worker", &["in"], &["out"]),
            ],
            edges: vec![make_edge("fe1", "body", "worker", "in")],
            loops: Vec::new(),
            prompt_required: true,
        };

        let state = empty_run_state();
        let ready = ready_nodes(&pipeline, &state);
        assert!(
            !ready.contains(&"fe1".to_string()),
            "ForEach should not appear in ready_nodes"
        );
    }

    // --- foreach_resolve_collection tests (issue #65) ---

    #[test]
    fn foreach_resolve_collection_returns_list_for_matching_field() {
        let mut fm = HashMap::new();
        fm.insert(
            "issues".into(),
            serde_yaml::Value::Sequence(vec![
                serde_yaml::Value::String("a".into()),
                serde_yaml::Value::String("b".into()),
            ]),
        );
        let result = foreach_resolve_collection(&fm, "issues");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn foreach_resolve_collection_returns_empty_for_missing_field() {
        let fm = HashMap::new();
        let result = foreach_resolve_collection(&fm, "issues");
        assert!(result.is_empty());
    }

    #[test]
    fn foreach_resolve_collection_returns_empty_for_wrong_type() {
        let mut fm = HashMap::new();
        fm.insert(
            "issues".into(),
            serde_yaml::Value::String("not-a-list".into()),
        );
        let result = foreach_resolve_collection(&fm, "issues");
        assert!(result.is_empty());
    }

    #[test]
    fn foreach_with_over_field_reads_named_frontmatter_field() {
        let pipeline = PipelineDef {
            name: "foreach-over".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("upstream", &["in"], &["out"]),
                make_foreach_node_with_over("fe1", "tasks"),
                make_node("worker", &["in"], &["out"]),
                make_end_node(),
            ],
            edges: vec![
                make_edge("upstream", "out", "fe1", "in"),
                make_edge("fe1", "body", "worker", "in"),
                make_edge("fe1", "done", "end", "result"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("upstream".into(), completed_node("upstream"));

        let mut frontmatter = HashMap::new();
        frontmatter.insert(
            "tasks".into(),
            serde_yaml::Value::Sequence(vec![
                serde_yaml::Value::String("t1".into()),
                serde_yaml::Value::String("t2".into()),
            ]),
        );

        let actions = evaluate_outgoing_edges_with_context(
            &pipeline,
            &state,
            "upstream",
            &HashMap::new(),
            &frontmatter,
        );

        assert!(actions.contains(&SchedulerAction::ForEachStarted {
            foreach_node_id: "fe1".into(),
            total_items: 2,
            items: vec![
                serde_yaml::Value::String("t1".into()),
                serde_yaml::Value::String("t2".into()),
            ],
        }));
        for i in 1..=2 {
            assert!(
                actions.contains(&SchedulerAction::Spawn {
                    node_id: "worker".into(),
                    iter: i,
                }),
                "should spawn worker iter {i}"
            );
        }
    }

    #[test]
    fn foreach_without_over_falls_back_to_items() {
        let pipeline = PipelineDef {
            name: "foreach-fallback".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("upstream", &["in"], &["out"]),
                make_foreach_node("fe1"),
                make_node("worker", &["in"], &["out"]),
                make_end_node(),
            ],
            edges: vec![
                make_edge("upstream", "out", "fe1", "in"),
                make_edge("fe1", "body", "worker", "in"),
                make_edge("fe1", "done", "end", "result"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("upstream".into(), completed_node("upstream"));

        let mut frontmatter = HashMap::new();
        frontmatter.insert(
            "items".into(),
            serde_yaml::Value::Sequence(vec![serde_yaml::Value::String("x".into())]),
        );

        let actions = evaluate_outgoing_edges_with_context(
            &pipeline,
            &state,
            "upstream",
            &HashMap::new(),
            &frontmatter,
        );

        assert!(actions.contains(&SchedulerAction::ForEachStarted {
            foreach_node_id: "fe1".into(),
            total_items: 1,
            items: vec![serde_yaml::Value::String("x".into())],
        }));
    }

    // --- Layer 3a: integration test — parse YAML + schedule (issue #65) ---

    #[test]
    fn integration_foreach_over_issues_with_typed_upstream() {
        let yaml = r#"
name: foreach-integration
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: ab000001
    name: lister
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: plan
        frontmatter:
          issues:
            type: list
  - id: ab000002
    name: per-issue
    type: for-each
    over: issues
    inputs:
      - name: in
      - name: break
    outputs:
      - name: body
      - name: done
  - id: ab000003
    name: worker
    type: code-mutating
    inputs:
      - name: in
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: ab000001, port: task }
  - source: { node: ab000001, port: plan }
    target: { node: ab000002, port: in }
  - source: { node: ab000002, port: body }
    target: { node: ab000003, port: in }
  - source: { node: ab000002, port: done }
    target: { node: end, port: result }
"#;
        let result = crate::pipeline::parse_pipeline(yaml).unwrap();
        let pipeline = result.pipeline;

        let fe = pipeline
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::ForEach)
            .unwrap();
        assert_eq!(fe.over.as_deref(), Some("issues"));

        let mut state = empty_run_state();
        state
            .nodes
            .insert("ab000001".into(), completed_node("ab000001"));

        let mut frontmatter = HashMap::new();
        frontmatter.insert(
            "issues".into(),
            serde_yaml::Value::Sequence(vec![
                serde_yaml::Value::String("a".into()),
                serde_yaml::Value::String("b".into()),
                serde_yaml::Value::String("c".into()),
            ]),
        );

        let actions = evaluate_outgoing_edges_with_context(
            &pipeline,
            &state,
            "ab000001",
            &HashMap::new(),
            &frontmatter,
        );

        assert!(
            actions.contains(&SchedulerAction::ForEachStarted {
                foreach_node_id: "ab000002".into(),
                total_items: 3,
                items: vec![
                    serde_yaml::Value::String("a".into()),
                    serde_yaml::Value::String("b".into()),
                    serde_yaml::Value::String("c".into()),
                ],
            }),
            "3 issues should produce ForEachStarted with total_items=3"
        );
        for i in 1..=3 {
            assert!(
                actions.contains(&SchedulerAction::Spawn {
                    node_id: "ab000003".into(),
                    iter: i,
                }),
                "should spawn worker iter {i}"
            );
        }
    }

    #[test]
    fn integration_foreach_over_missing_field_fires_empty() {
        let yaml = r#"
name: foreach-missing
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: ab000001
    name: lister
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: plan
  - id: ab000002
    name: per-issue
    type: for-each
    over: nonexistent
    inputs:
      - name: in
      - name: break
    outputs:
      - name: body
      - name: done
  - id: ab000003
    name: worker
    type: code-mutating
    inputs:
      - name: in
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: ab000001, port: task }
  - source: { node: ab000001, port: plan }
    target: { node: ab000002, port: in }
  - source: { node: ab000002, port: body }
    target: { node: ab000003, port: in }
  - source: { node: ab000002, port: done }
    target: { node: end, port: result }
"#;
        let result = crate::pipeline::parse_pipeline(yaml).unwrap();
        let pipeline = result.pipeline;

        let mut state = empty_run_state();
        state
            .nodes
            .insert("ab000001".into(), completed_node("ab000001"));

        let frontmatter: HashMap<String, serde_yaml::Value> = [(
            "items".into(),
            serde_yaml::Value::Sequence(vec![serde_yaml::Value::String("a".into())]),
        )]
        .into_iter()
        .collect();

        let actions = evaluate_outgoing_edges_with_context(
            &pipeline,
            &state,
            "ab000001",
            &HashMap::new(),
            &frontmatter,
        );

        assert!(
            actions.contains(&SchedulerAction::ForEachEmpty {
                foreach_node_id: "ab000002".into(),
            }),
            "over: nonexistent should resolve to empty list and fire ForEachEmpty"
        );
        assert!(
            actions.contains(&SchedulerAction::ForEachDone {
                foreach_node_id: "ab000002".into(),
            }),
            "empty foreach should fire done immediately"
        );
    }

    // ── Bounded loop REGION iteration (ADR-0011 / #148) ──────────────────────
    //
    // The bounded-region review loop migrated from Loop+Switch: the body is the
    // `loops:` region [impl, rev]; routing lives on the edges (rev -> end WHEN
    // verdict in [PASS], rev -> impl ELSE). These tests pin the scheduler's
    // runtime wiring for region iteration — the seam the L5 manager-unstick
    // scenario exercises and which had no daemon-level coverage.

    fn migrated_review_loop_pipeline(max_iter: i64) -> PipelineDef {
        PipelineDef {
            name: "manager-unstick-loop".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("start", &[], &["user_prompt"]),
                make_node("impl", &["task", "review"], &["code"]),
                make_node("rev", &["code"], &["review"]),
                make_end_node(),
            ],
            edges: vec![
                make_edge("start", "user_prompt", "impl", "task"),
                make_edge("impl", "code", "rev", "code"),
                make_cond_edge(
                    "rev",
                    "review",
                    "end",
                    "result",
                    Some("verdict: { in: [PASS, APPROVED] }"),
                    false,
                ),
                make_cond_edge("rev", "review", "impl", "task", None, true),
            ],
            loops: vec![crate::pipeline::LoopRegion {
                id: "review_loop".into(),
                kind: crate::pipeline::LoopKind::Bounded,
                members: vec!["impl".into(), "rev".into()],
                max_iter: Some(serde_yaml::Value::Number(max_iter.into())),
                over: None,
            }],
            prompt_required: true,
        }
    }

    fn fail_fm() -> HashMap<String, HashMap<String, serde_yaml::Value>> {
        let mut rev_fm = HashMap::new();
        rev_fm.insert(
            "verdict".to_string(),
            serde_yaml::Value::String("FAIL".to_string()),
        );
        let mut by_node = HashMap::new();
        by_node.insert("rev".to_string(), rev_fm);
        by_node
    }

    #[test]
    fn region_back_edge_reenters_the_entry_at_the_next_lap() {
        // rev completes FAIL at lap 1 → the `else` back-edge rev->impl fires and
        // the region must re-enter: impl re-spawns at iter 2 (the next lap),
        // NOT halt "unrouted". Regression: the back-edge produced no re-entry
        // spawn because the region iteration was never tracked at runtime.
        let pipeline = migrated_review_loop_pipeline(2);
        let mut state = empty_run_state();
        state
            .nodes
            .insert("start".into(), completed_node_iter("start", 1));
        state
            .nodes
            .insert("impl".into(), completed_node_iter("impl", 1));
        state
            .nodes
            .insert("rev".into(), completed_node_iter("rev", 1));
        // Region tracked at lap 1.
        state.loop_states.insert(
            "review_loop".into(),
            crate::event_log::LoopState {
                loop_node_id: "review_loop".into(),
                current_iter: 1,
                max_iter: 2,
                break_received: false,
                done: false,
            },
        );

        let by_node = fail_fm();
        let actions = evaluate_outgoing_edges_full(
            &pipeline,
            &state,
            "rev",
            &HashMap::new(),
            by_node.get("rev").unwrap(),
            &by_node,
        );

        assert!(
            actions.contains(&SchedulerAction::Spawn {
                node_id: "impl".into(),
                iter: 2,
            }),
            "FAIL at lap 1 must re-enter impl at iter 2, got {actions:?}"
        );
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::Halt { .. })),
            "must not halt at lap 1, got {actions:?}"
        );
    }

    #[test]
    fn region_blocks_exhausted_unrouted_at_max_iter() {
        // rev completes FAIL at lap 2 == max_iter with no `iter >= max` exit edge
        // wired: the region must block the explicit "exhausted — unrouted" halt,
        // NOT re-enter (no iter-3 spawn) and NOT a generic unrouted message.
        let pipeline = migrated_review_loop_pipeline(2);
        let mut state = empty_run_state();
        state
            .nodes
            .insert("start".into(), completed_node_iter("start", 1));
        state
            .nodes
            .insert("impl".into(), completed_node_iter("impl", 2));
        state
            .nodes
            .insert("rev".into(), completed_node_iter("rev", 2));
        state.loop_states.insert(
            "review_loop".into(),
            crate::event_log::LoopState {
                loop_node_id: "review_loop".into(),
                current_iter: 2,
                max_iter: 2,
                break_received: false,
                done: false,
            },
        );

        let by_node = fail_fm();
        let actions = evaluate_outgoing_edges_full(
            &pipeline,
            &state,
            "rev",
            &HashMap::new(),
            by_node.get("rev").unwrap(),
            &by_node,
        );

        assert!(
            !actions.iter().any(|a| matches!(
                a,
                SchedulerAction::Spawn {
                    node_id,
                    iter: 3,
                } if node_id == "impl"
            )),
            "must not re-enter past max_iter, got {actions:?}"
        );
        let halt = actions.iter().find_map(|a| match a {
            SchedulerAction::Halt { message } => Some(message.clone()),
            _ => None,
        });
        let Some(halt) = halt else {
            panic!("expected an exhausted-unrouted halt, got {actions:?}");
        };
        assert!(
            halt.contains("exhausted") && halt.contains("unrouted"),
            "halt must be the region exhausted-unrouted reason, got {halt:?}"
        );
    }

    #[test]
    fn region_exits_early_on_pass_edge() {
        // rev PASSes at lap 1 → the guarded rev->end edge fires; the run
        // completes, leaving the region before max_iter. (No regression here;
        // pins the early-exit path stays intact alongside the re-entry fix.)
        let pipeline = migrated_review_loop_pipeline(2);
        let mut state = empty_run_state();
        state
            .nodes
            .insert("start".into(), completed_node_iter("start", 1));
        state
            .nodes
            .insert("impl".into(), completed_node_iter("impl", 1));
        state
            .nodes
            .insert("rev".into(), completed_node_iter("rev", 1));
        state.loop_states.insert(
            "review_loop".into(),
            crate::event_log::LoopState {
                loop_node_id: "review_loop".into(),
                current_iter: 1,
                max_iter: 2,
                break_received: false,
                done: false,
            },
        );

        let mut rev_fm = HashMap::new();
        rev_fm.insert(
            "verdict".to_string(),
            serde_yaml::Value::String("PASS".to_string()),
        );
        let mut by_node = HashMap::new();
        by_node.insert("rev".to_string(), rev_fm.clone());

        let actions = evaluate_outgoing_edges_full(
            &pipeline,
            &state,
            "rev",
            &HashMap::new(),
            &rev_fm,
            &by_node,
        );
        assert!(
            actions.contains(&SchedulerAction::Complete),
            "PASS must complete via rev->end, got {actions:?}"
        );
    }

    #[test]
    fn region_member_re_enters_then_forwards_to_next_member_at_the_new_lap() {
        // After the re-entry spawns impl at iter 2, impl completing must forward
        // (unconditional impl->rev) to spawn rev at iter 2 — the intra-body edge
        // is NOT a region re-entry, so it takes the generic forward path. This is
        // what stamps both members at the region iter, which the run overlay
        // reads to render the exhausted-unrouted affordance.
        let pipeline = migrated_review_loop_pipeline(2);
        let mut state = empty_run_state();
        state
            .nodes
            .insert("start".into(), completed_node_iter("start", 1));
        // impl has re-entered and completed at lap 2; rev is still at lap 1.
        state
            .nodes
            .insert("impl".into(), completed_node_iter("impl", 2));
        state
            .nodes
            .insert("rev".into(), completed_node_iter("rev", 1));
        state.loop_states.insert(
            "review_loop".into(),
            crate::event_log::LoopState {
                loop_node_id: "review_loop".into(),
                current_iter: 2,
                max_iter: 2,
                break_received: false,
                done: false,
            },
        );

        let actions = evaluate_outgoing_edges_full(
            &pipeline,
            &state,
            "impl",
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert!(
            actions.contains(&SchedulerAction::Spawn {
                node_id: "rev".into(),
                iter: 2,
            }),
            "impl@2 must forward to rev@2 on the new lap, got {actions:?}"
        );
    }

    // ── Canonical upstream preconditions (#194 / #210) ───────────────────────
    //
    // A forward spawn's preconditions consider only *forward* edges. A
    // self-edge can never be satisfied before the node's own first run; a
    // region back-edge belongs to the region engine (`handle_region_reentry`).
    // Counting either as an upstream blocker reproduces the forensic
    // run-9c8d123 stall: zero events, run sits Running forever.

    #[test]
    fn self_edge_is_not_an_upstream_precondition() {
        // Forensic self-edge (ecbJixkS.screens-fixed -> ecbJixkS.in) drawn
        // outside any region: when the real upstream completes, the node must
        // spawn — never a silent stall on its own output.
        let pipeline = PipelineDef {
            name: "self-edge".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("griller", &["task"], &["agentic_test"]),
                make_node("tester", &["test", "screens"], &["screens_fixed"]),
                make_end_node(),
            ],
            edges: vec![
                make_edge("griller", "agentic_test", "tester", "test"),
                make_edge("tester", "screens_fixed", "tester", "screens"),
                make_end_edge("tester", "screens_fixed", "done"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("griller".into(), completed_node("griller"));

        let actions = evaluate_outgoing_edges_full(
            &pipeline,
            &state,
            "griller",
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );

        assert!(
            actions.contains(&SchedulerAction::Spawn {
                node_id: "tester".into(),
                iter: 1,
            }),
            "tester must spawn when its real upstream completed; the self-edge \
             is not a precondition, got {actions:?}"
        );
    }

    #[test]
    fn region_entry_join_spawns_on_external_feeder_completion() {
        // The region entry (impl) is fed by an external feeder AND by the
        // rev->impl back-edge. When the feeder completes, the entry spawns at
        // lap 1: the back-edge is the region engine's concern, not a forward
        // precondition (#194 loop-entry join stall).
        let pipeline = migrated_review_loop_pipeline(3);

        let mut state = empty_run_state();
        state.nodes.insert("start".into(), completed_node("start"));

        let actions = evaluate_outgoing_edges_full(
            &pipeline,
            &state,
            "start",
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );

        assert!(
            actions.contains(&SchedulerAction::Spawn {
                node_id: "impl".into(),
                iter: 1,
            }),
            "region entry must spawn on feeder completion without waiting on \
             its back-edge, got {actions:?}"
        );
    }

    // ── Region closure (#199 / #210) ─────────────────────────────────────────

    fn region_state(current_iter: i64, max_iter: i64, done: bool) -> crate::event_log::LoopState {
        crate::event_log::LoopState {
            loop_node_id: "review_loop".into(),
            current_iter,
            max_iter,
            break_received: false,
            done,
        }
    }

    #[test]
    fn ended_region_closes_instead_of_starting_a_phantom_lap() {
        // #199 forensic: `end_region` on an active bounded region started a
        // new lap (entry re-spawned at iter 4 > max_iter 3). An ended region
        // must route its exit (or halt unrouted) at the current iter — never
        // re-spawn the entry, never bump the counter.
        let pipeline = migrated_review_loop_pipeline(3);
        let mut state = empty_run_state();
        state
            .nodes
            .insert("start".into(), completed_node_iter("start", 1));
        state
            .nodes
            .insert("impl".into(), completed_node_iter("impl", 1));
        state
            .nodes
            .insert("rev".into(), completed_node_iter("rev", 1));
        // end_region projected: region closed at lap 1 (< max 3).
        state
            .loop_states
            .insert("review_loop".into(), region_state(1, 3, true));

        let by_node = fail_fm();
        let actions = evaluate_outgoing_edges_full(
            &pipeline,
            &state,
            "rev",
            &HashMap::new(),
            by_node.get("rev").unwrap(),
            &by_node,
        );

        assert!(
            !actions.iter().any(|a| matches!(
                a,
                SchedulerAction::Spawn { node_id, .. } if node_id == "impl"
            )),
            "an ended region must never re-spawn its entry, got {actions:?}"
        );
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::LoopIterStarted { .. })),
            "an ended region must not advance its lap counter, got {actions:?}"
        );
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::Halt { .. })),
            "ended with no matching exit edge: explicit halt, never a silent \
             stall, got {actions:?}"
        );
    }

    #[test]
    fn forward_reevaluation_never_spawns_a_member_past_max_iter() {
        // #199 forensic: after end_region, re-evaluation replayed the feeder's
        // forward edge into the region entry and spawned it at iter 4 with
        // max_iter 3. No code path may push a member past the region bound.
        let pipeline = migrated_review_loop_pipeline(3);
        let mut state = empty_run_state();
        state
            .nodes
            .insert("start".into(), completed_node_iter("start", 1));
        state
            .nodes
            .insert("impl".into(), completed_node_iter("impl", 3));
        state
            .nodes
            .insert("rev".into(), completed_node_iter("rev", 3));
        state
            .loop_states
            .insert("review_loop".into(), region_state(3, 3, true));

        // Re-evaluation pass replays the feeder's outgoing edges.
        let actions = evaluate_outgoing_edges_full(
            &pipeline,
            &state,
            "start",
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );

        assert!(
            !actions.iter().any(|a| matches!(
                a,
                SchedulerAction::Spawn { node_id, iter } if node_id == "impl" && *iter > 3
            )),
            "a member must never spawn past max_iter, got {actions:?}"
        );
    }

    #[test]
    fn completed_non_member_is_never_respawned_by_forward_reevaluation() {
        // #199 / #195 forensic: the griller — NOT a member of the region — was
        // re-spawned at iter 4 by the lap bump. A completed node reached only
        // by forward edges must never be re-run by re-evaluation; only a
        // back-edge (emergent cycle) or a region lap may re-run a node.
        let pipeline = PipelineDef {
            name: "feeder-chain".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("start", &[], &["user_prompt"]),
                make_node("griller", &["task"], &["plan"]),
                make_node("impl", &["plan"], &["code"]),
                make_end_node(),
            ],
            edges: vec![
                make_edge("start", "user_prompt", "griller", "task"),
                make_edge("griller", "plan", "impl", "plan"),
                make_end_edge("impl", "code", "done"),
            ],
            loops: Vec::new(),
            prompt_required: true,
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("start".into(), completed_node_iter("start", 1));
        state
            .nodes
            .insert("griller".into(), completed_node_iter("griller", 1));

        // Re-evaluation replays start's outgoing edges; griller already ran.
        let actions = evaluate_outgoing_edges_full(
            &pipeline,
            &state,
            "start",
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );

        assert!(
            !actions.iter().any(|a| matches!(
                a,
                SchedulerAction::Spawn { node_id, .. } if node_id == "griller"
            )),
            "a completed non-member must never be re-spawned by forward \
             re-evaluation, got {actions:?}"
        );
    }

    // ── #172: entering a bounded region from outside ──────────────────────────
    //
    // Topology that the default `bugfix` pipeline exhibits and that deadlocked
    // silently before the fix:
    //
    //   dbg ──(verdict eq Bug)──▶ impl ⇄ tst
    //   dbg ──(repro, context)──▶ tst        impl ──▶ tst (forward)
    //                                        tst  ──▶ impl (back-edge / else)
    //                                        tst  ──(verdict eq Pass)──▶ end
    //
    // Bounded region [impl, tst]; entry = impl (first member with an external
    // incoming edge). The back-edge tst->impl is a region re-entry edge: it must
    // NOT count as an upstream precondition for impl's first spawn, or impl never
    // starts — its only other producer, tst, sits downstream of impl in the cycle
    // and can never complete first. ADR-0011: no silent stall.
    fn external_entry_into_loop_pipeline(max_iter: i64) -> PipelineDef {
        PipelineDef {
            name: "external-entry-loop".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("dbg", &["task"], &["verdict", "repro"]),
                make_node("impl", &["task", "review"], &["code"]),
                make_node("tst", &["code", "repro"], &["verdict"]),
                make_end_node(),
            ],
            edges: vec![
                // External forward edge into the loop entry, guarded.
                make_cond_edge(
                    "dbg",
                    "verdict",
                    "impl",
                    "task",
                    Some("verdict: { eq: Bug }"),
                    false,
                ),
                // External context edge into the *other* member (not the entry).
                make_edge("dbg", "repro", "tst", "repro"),
                // Intra-body forward edge.
                make_edge("impl", "code", "tst", "code"),
                // Region exit (guarded) and back-edge (else) — both off `tst`.
                make_cond_edge(
                    "tst",
                    "verdict",
                    "end",
                    "result",
                    Some("verdict: { eq: Pass }"),
                    false,
                ),
                make_cond_edge("tst", "verdict", "impl", "review", None, true),
            ],
            loops: vec![crate::pipeline::LoopRegion {
                id: "fix_loop".into(),
                kind: crate::pipeline::LoopKind::Bounded,
                members: vec!["impl".into(), "tst".into()],
                max_iter: Some(serde_yaml::Value::Number(max_iter.into())),
                over: None,
            }],
            prompt_required: true,
        }
    }

    #[test]
    fn external_forward_edge_spawns_bounded_loop_entry() {
        // dbg completes with verdict=Bug → the guarded entry edge dbg->impl fires.
        // impl is the region entry and also the target of the back-edge tst->impl.
        // The back-edge must be excluded from impl's upstream join, so impl spawns
        // at iter 1 on dbg's completion alone. (Before the fix: no spawn, no halt,
        // run stuck `running` forever — #172.)
        let pipeline = external_entry_into_loop_pipeline(3);
        let mut state = empty_run_state();
        state.nodes.insert("dbg".into(), completed_node("dbg"));

        let mut dbg_fm = HashMap::new();
        dbg_fm.insert(
            "verdict".to_string(),
            serde_yaml::Value::String("Bug".to_string()),
        );
        let mut by_node = HashMap::new();
        by_node.insert("dbg".to_string(), dbg_fm.clone());

        let actions = evaluate_outgoing_edges_full(
            &pipeline,
            &state,
            "dbg",
            &HashMap::new(),
            &dbg_fm,
            &by_node,
        );

        assert!(
            actions.contains(&SchedulerAction::Spawn {
                node_id: "impl".into(),
                iter: 1,
            }),
            "entering the loop from dbg must spawn the entry impl@1, got {actions:?}"
        );
        // The context edge fired too, but tst must wait for impl (its forward
        // producer), so it does NOT spawn yet — and nothing halts silently.
        assert!(
            !actions.iter().any(|a| matches!(
                a,
                SchedulerAction::Spawn { node_id, .. } if node_id == "tst"
            )),
            "tst must wait for impl, not spawn on dbg's completion, got {actions:?}"
        );
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::Halt { .. })),
            "entering a bounded loop must not halt, got {actions:?}"
        );
    }

    #[test]
    fn bounded_loop_entry_then_forwards_to_second_member() {
        // After impl spawns and completes its first lap, its forward edge
        // impl->tst must spawn tst@1: tst's upstream is {dbg (done), impl (just
        // completed)} — the back-edge is excluded, so the join resolves.
        let pipeline = external_entry_into_loop_pipeline(3);
        let mut state = empty_run_state();
        state.nodes.insert("dbg".into(), completed_node("dbg"));
        state.nodes.insert("impl".into(), completed_node("impl"));

        let actions = evaluate_outgoing_edges_full(
            &pipeline,
            &state,
            "impl",
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );

        assert!(
            actions.contains(&SchedulerAction::Spawn {
                node_id: "tst".into(),
                iter: 1,
            }),
            "impl completing must forward to spawn tst@1, got {actions:?}"
        );
    }
}
