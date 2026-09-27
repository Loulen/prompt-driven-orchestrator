//! What a completed node's outgoing edges ask the scheduler to do: spawn, route
//! a switch, enter or re-enter a region, halt, interrupt or complete the Run.

use super::*;

pub fn evaluate_outgoing_edges_with_context(
    pipeline: &PipelineDef,
    run_state: &RunState,
    completed_node_id: &str,
    resolved_vars: &HashMap<String, serde_yaml::Value>,
    frontmatter_fields: &HashMap<String, serde_yaml::Value>,
) -> Vec<SchedulerAction> {
    // Seed the per-node map with the completed node's own frontmatter, or
    // convergence suppression cannot re-evaluate this producer's edges. Other
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
/// classifier feeding a suppressed `else` branch). THE canonical scheduler entry
/// point — call it per completed producer.
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

    // ADR-0011: a `force_route` short-circuits this node's `when:` edges AND the
    // `unrouted` detection below — the point is to unstick a run wedged because no
    // live branch matched. Endpoints are validated by the handler before append,
    // so the target always exists.
    if let Some(target) = run_state.forced_routes.get(completed_node_id) {
        let end_node_id = pipeline
            .nodes
            .iter()
            .find(|n| n.node_type == NodeType::End)
            .map(|n| n.id.as_str());
        if end_node_id == Some(target.as_str()) {
            actions.push(SchedulerAction::Complete);
        } else {
            actions.push(SchedulerAction::Spawn {
                node_id: target.clone(),
                iter: 1,
            });
        }
        return actions;
    }

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

    // Conditional routing on edges (ADR-0011), multi-match: every edge whose
    // `when:` is satisfied fires; an `else` edge fires iff no sibling on the same
    // source port matched. Switch nodes keep their own port-based routing via
    // `matched_port` for backward compatibility.
    let fired_indices: HashSet<usize> = if is_switch {
        // Switch routing is handled by `matched_port`; don't double-gate.
        HashSet::new()
    } else {
        let outgoing: Vec<(usize, &pdo_pipeline::domain::pipeline::EdgeDef)> = pipeline
            .edges
            .iter()
            .enumerate()
            .filter(|(_, e)| e.source.node == completed_node_id)
            .collect();
        let edge_refs: Vec<&pdo_pipeline::domain::pipeline::EdgeDef> =
            outgoing.iter().map(|(_, e)| *e).collect();
        let fired =
            edge_router::fired_edges(&edge_refs, frontmatter_fields, resolved_vars, source_iter);
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
            if !edge.source.carries(port) {
                continue;
            }
        }

        if !is_switch && !fired_indices.contains(&edge_index) {
            continue;
        }

        let target_id = &edge.target.node;

        // A member→non-member edge of a `collection` region is a BARRIER exit,
        // owned by `evaluate_collection_barrier`. Don't let the generic path act
        // here: it would spawn downstream / complete the run after the FIRST item.
        if let Some(region) =
            crate::domain::loop_region::collection_region_for_member(pipeline, completed_node_id)
        {
            if !region.members.iter().any(|m| m == target_id) {
                continue;
            }
        }

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
            } else if check_all_upstream_completed(
                pipeline,
                run_state,
                target_id,
                completed_node_id,
                frontmatter_by_node,
                resolved_vars,
            ) {
                // `End` is a CONVERGENCE BARRIER, not first-past-the-post: don't
                // complete on the first inbound edge. In a flat parallel fan-out
                // that flipped the whole run `completed` when the fast branch
                // arrived, stranding the sibling `running` with no way back (its
                // late `pdo complete` → 409, and `resume_run` is a no-op on a
                // terminal run). Gate like a `Merge`: complete only once EVERY
                // inbound edge is resolved (source completed, is the just-completed
                // node, or is a dead branch — so a suppressed conditional path
                // never stalls the run).
                actions.push(SchedulerAction::Complete);
            }
            // else: a sibling edge into `End` is still live. Suppressing keeps `End`
            // unreached, so `is_node_dead` stays false and the unrouted-convergence
            // check below does not misfire.
        } else if let Some(region) = crate::domain::loop_region::bounded_region_reentered_by_edge(
            pipeline,
            completed_node_id,
            target_id,
        ) {
            // Region back-edge (member -> entry): the region engine, not the
            // generic forward-spawn path, governs re-entry — otherwise the entry
            // would be spawned once per fired back-edge and past the bound.
            actions.extend(handle_region_reentry(
                pipeline,
                run_state,
                region,
                target_id,
                source_iter,
                frontmatter_fields,
                resolved_vars,
            ));
        } else if let Some(region) = crate::domain::loop_region::collection_region_entered_by_edge(
            pipeline,
            completed_node_id,
            target_id,
        ) {
            // Entry into a `collection` region from outside: the region engine,
            // not the generic forward-spawn path, owns the spawn — it fans the
            // entry out once per item of the frontmatter's `over` list.
            actions.extend(handle_collection_entry(
                pipeline,
                run_state,
                region,
                frontmatter_fields,
            ));
        } else {
            let target_node = pipeline.nodes.iter().find(|n| n.id == *target_id);
            let is_loop_target = target_node.is_some_and(|n| n.node_type == NodeType::Loop);

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
                        // A bounded region gets its `loop_states` entry from lap 1,
                        // so "no loop_states entry" means "no loop" and never
                        // "first lap" (ADR-0025 §4). The absent-loop-state guard is
                        // what keeps re-processing this producer from double-seeding.
                        if let Some(region) =
                            crate::domain::loop_region::bounded_region_entered_by_edge(
                                pipeline,
                                completed_node_id,
                                target_id,
                            )
                        {
                            if !run_state.loop_states.contains_key(region.id.as_str())
                                && !actions.iter().any(|a| {
                                    matches!(
                                        a,
                                        SchedulerAction::LoopIterStarted { loop_node_id, .. }
                                            if loop_node_id == &region.id
                                    )
                                })
                            {
                                actions.push(SchedulerAction::LoopIterStarted {
                                    loop_node_id: region.id.clone(),
                                    iter: 1,
                                    max_iter: crate::domain::loop_region::resolve_region_max_iter(
                                        region,
                                        resolved_vars,
                                    ),
                                });
                            }
                        }
                        // #785: two edges of this producer wired to the SAME target
                        // (e.g. `out` and `Chosen design` both → the loop input) must
                        // yield ONE `Spawn`, exactly as the adjacent `LoopIterStarted`
                        // is deduplicated. Without this, the completion path (which
                        // runs `SpawnDedup::InternalOnly`) drove two spawns of the
                        // same `(node, iter)`: the loser hit tmux `duplicate session`
                        // and parked the run `spawn_aborted` over a live winner.
                        let spawn = SchedulerAction::Spawn {
                            node_id: target_id.clone(),
                            iter: next_iter,
                        };
                        if !actions.contains(&spawn) {
                            actions.push(spawn);
                        }
                    }
                }
            }
        }
    }

    // Explicit halt on unrouted convergence (ADR-0011, "jamais de stall
    // silencieux"). A convergence whose branches are ALL dead is never spawned and
    // becomes dead itself; the cascade can render `End` unreachable, and the run
    // would sit `Running` forever. Park instead, so the state is diagnosable.
    //
    // Only when this completion produced no forward progress: if `End` is still
    // reachable through a live path, a Merge waiting on a running sibling is
    // normal, not a stall.
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
                // Don't just say "unrouted": the operator needs each guard, whether
                // it fired, and the value actually read, to know where to
                // `force_route`.
                let candidates = describe_candidate_edges(
                    pipeline,
                    completed_node_id,
                    source_iter,
                    &fired_indices,
                    frontmatter_fields,
                );
                actions.push(SchedulerAction::Interrupt {
                    reason_code: "unrouted".to_string(),
                    message: format!(
                        "unrouted: node '{completed_node_id}' (iter {source_iter}) completed \
                         but conditional routing suppressed every path to End (no live branch \
                         reaches End). Candidate edges:\n{candidates}\n\
                         Route it explicitly with force_route (source '{completed_node_id}' \
                         -> a target), or adjust the verdict the edges read."
                    ),
                });
            }
        }
    }

    actions
}
