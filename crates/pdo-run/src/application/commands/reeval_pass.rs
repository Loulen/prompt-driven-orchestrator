//! One pass of the post-command re-evaluation: the scheduler's evaluators over
//! the fresh projection, their actions through the one interpreter.

use super::*;

/// The re-evaluation proper: load the pipeline and run state, resolve variables
/// (including cycle extensions), then re-evaluate the outgoing edges of every
/// completed node. Returns what actually happened, so handlers can tell the truth.
pub(super) async fn re_evaluate_after_command_inner(state: &Runs, run_id: &str) -> ReEvalSummary {
    let mut summary = ReEvalSummary::default();
    let events = match load_events(state, run_id).await {
        Ok(e) => e,
        Err(e) => {
            error!("re_evaluate_after_command: failed to load events: {e}");
            summary.skipped.push(format!("failed to load events: {e}"));
            return summary;
        }
    };
    let run_state = match projection::project(&events) {
        Some(s) => s,
        None => {
            summary.skipped.push("run not found".to_string());
            return summary;
        }
    };

    let repo_root = effective_repo_root(state, &run_state);
    let pipeline_path = {
        let run_scoped = run_scoped_pipeline_path(&repo_root, run_id);
        if run_scoped.exists() {
            run_scoped
        } else {
            resolve_pipeline_path(&repo_root, &run_state.pipeline_name)
        }
    };
    let Ok(yaml) = std::fs::read_to_string(&pipeline_path) else {
        summary.skipped.push("pipeline file unreadable".to_string());
        return summary;
    };
    let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
        summary.skipped.push("pipeline failed to parse".to_string());
        return summary;
    };

    let pipeline = parse_result.pipeline;
    let worktree_dir = worktree_dir_for_run(&repo_root, run_id);
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
    let mut resolved_vars = resolve_run_variables(&pipeline, &events);

    // For each extend_cycle command, bump the variables referenced by the target
    // node's outgoing edges.
    let extensions = projection::collect_cycle_extensions(&events);
    for (ext_node_id, additional) in &extensions {
        let var_refs = extract_variable_refs_from_outgoing_edges(&pipeline, ext_node_id);
        for var_name in var_refs {
            if let Some(val) = resolved_vars.get_mut(&var_name) {
                if let Some(n) = val.as_i64() {
                    *val = serde_yaml::Value::Number(serde_yaml::Number::from(n + additional));
                }
            }
        }
    }

    // A `bump_region` raises the region's effective `max_iter`. When that cap is a
    // `$var`, bumping the variable is what lifts the `iter >= max` exit guard so the
    // region runs the extra laps; a literal cap is the region engine's own bound and
    // reads the recorded route directly.
    let region_routes = projection::collect_region_routes(&events);
    for (region_id, route) in &region_routes {
        if route.bumped_by <= 0 {
            continue;
        }
        if let Some(region) = pipeline.loops.iter().find(|r| &r.id == region_id) {
            if let Some(serde_yaml::Value::String(s)) = &region.max_iter {
                if let Some(var_name) = s.strip_prefix('$') {
                    if let Some(val) = resolved_vars.get_mut(var_name) {
                        if let Some(n) = val.as_i64() {
                            *val = serde_yaml::Value::Number(serde_yaml::Number::from(
                                n + route.bumped_by,
                            ));
                        }
                    }
                }
            }
        }
    }

    // Settled-complete nodes whose edges might now fire with updated vars. A
    // `Skipped` node counts too: its edges re-evaluate exactly like a `Completed`
    // node's.
    let completed_node_ids: Vec<String> = run_state
        .nodes
        .values()
        .filter(|n| n.status.is_settled_complete())
        .map(|n| n.node_id.clone())
        .collect();

    let spawn_ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: &worktree_dir,
        artifacts_dir: &artifacts_dir,
        resolved_vars: &resolved_vars,
        repo_root: &repo_root,
    };

    let frontmatter_by_node = resolve_completed_frontmatter(&pipeline, &run_state, &artifacts_dir);
    // A capped region's Coût de boucle is the scheduler's input at re-entry: a
    // `resume_run` on a Run parked « budget reached » re-parks it until the cap
    // is raised, the exit `force_route`d or the region ended.
    let scheduler_state = crate::application::scheduler::with_loop_costs_for_caps(
        state, &run_state, &events, &pipeline,
    )
    .await;

    for completed_node_id in &completed_node_ids {
        let source_iter = run_state
            .nodes
            .get(completed_node_id)
            .map(|n| n.iter)
            .unwrap_or(1);

        let frontmatter_fields =
            resolve_source_frontmatter(&pipeline, completed_node_id, source_iter, &artifacts_dir);

        let actions = scheduler::evaluate_outgoing_edges_full(
            &pipeline,
            &scheduler_state,
            completed_node_id,
            &resolved_vars,
            &frontmatter_fields,
            &frontmatter_by_node,
        );

        for action in &actions {
            // `GuardSuperfluous`: schedule only MISSING work — never a node with a
            // live iteration, never a completed one — on the pass-1 snapshot (INV-2).
            match scheduler_interpreter::interpret(
                state,
                &spawn_ctx,
                &run_state,
                SpawnDedup::GuardSuperfluous,
                source_iter,
                action,
            )
            .await
            {
                ActionOutcome::Spawned {
                    node_id,
                    iter,
                    outcome,
                } => summary.record_spawn(&node_id, iter, outcome),
                ActionOutcome::SpawnSkipped { reason } => {
                    info!("re_evaluate_after_command: skip spawn — {reason}");
                    summary.skipped.push(reason);
                }
                ActionOutcome::Progressed => {}
                ActionOutcome::Completed => {
                    summary.terminal = Some(ReEvalTerminal::Completed);
                    return summary;
                }
                ActionOutcome::Halted { message } => {
                    summary.terminal = Some(ReEvalTerminal::Halted(message));
                    return summary;
                }
                ActionOutcome::Interrupted { message } => {
                    summary.terminal = Some(ReEvalTerminal::Interrupted(message));
                    return summary;
                }
            }
        }
    }

    // Pass 1 may have appended events; re-project so pass 2 sees fresh state.
    let Some((fresh_events, fresh_run_state)) = reload_run_state(state, run_id).await else {
        return summary;
    };
    let mut fresh_resolved_vars = resolve_run_variables(&pipeline, &fresh_events);
    for (ext_node_id, additional) in &extensions {
        let var_refs = extract_variable_refs_from_outgoing_edges(&pipeline, ext_node_id);
        for var_name in var_refs {
            if let Some(val) = fresh_resolved_vars.get_mut(&var_name) {
                if let Some(n) = val.as_i64() {
                    *val = serde_yaml::Value::Number(serde_yaml::Number::from(n + additional));
                }
            }
        }
    }
    let fresh_spawn_ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: &worktree_dir,
        artifacts_dir: &artifacts_dir,
        resolved_vars: &fresh_resolved_vars,
        repo_root: &repo_root,
    };

    for loop_node in pipeline
        .nodes
        .iter()
        .filter(|n| n.node_type == pipeline::NodeType::Loop)
    {
        let loop_actions = scheduler::evaluate_loop_body_completion(
            &pipeline,
            &fresh_run_state,
            &loop_node.id,
            &fresh_resolved_vars,
        );
        // GuardSuperfluous on the reloaded snapshot (INV-2); source_iter is
        // irrelevant here, since no SwitchRouted is emitted (INV-3).
        for action in &loop_actions {
            match scheduler_interpreter::interpret(
                state,
                &fresh_spawn_ctx,
                &fresh_run_state,
                SpawnDedup::GuardSuperfluous,
                1,
                action,
            )
            .await
            {
                ActionOutcome::Spawned {
                    node_id,
                    iter,
                    outcome,
                } => summary.record_spawn(&node_id, iter, outcome),
                ActionOutcome::SpawnSkipped { reason } => {
                    info!("re_evaluate_after_command(loop): skip spawn — {reason}");
                    summary.skipped.push(reason);
                }
                ActionOutcome::Progressed => {}
                ActionOutcome::Completed => {
                    summary.terminal = Some(ReEvalTerminal::Completed);
                    return summary;
                }
                ActionOutcome::Halted { message } => {
                    summary.terminal = Some(ReEvalTerminal::Halted(message));
                    return summary;
                }
                ActionOutcome::Interrupted { message } => {
                    summary.terminal = Some(ReEvalTerminal::Interrupted(message));
                    return summary;
                }
            }
        }
    }

    let fresh_frontmatter_by_node =
        resolve_completed_frontmatter(&pipeline, &fresh_run_state, &artifacts_dir);
    for region in pipeline
        .loops
        .iter()
        .filter(|r| r.kind == pipeline::LoopKind::Collection)
    {
        let collection_actions = scheduler::evaluate_collection_barrier(
            &pipeline,
            &fresh_run_state,
            region,
            &fresh_frontmatter_by_node,
            &fresh_resolved_vars,
        );
        // GuardSuperfluous on the reloaded snapshot; source_iter irrelevant, since
        // no SwitchRouted is emitted.
        for action in &collection_actions {
            match scheduler_interpreter::interpret(
                state,
                &fresh_spawn_ctx,
                &fresh_run_state,
                SpawnDedup::GuardSuperfluous,
                1,
                action,
            )
            .await
            {
                ActionOutcome::Spawned {
                    node_id,
                    iter,
                    outcome,
                } => summary.record_spawn(&node_id, iter, outcome),
                ActionOutcome::SpawnSkipped { reason } => {
                    info!("re_evaluate_after_command(collection): skip spawn — {reason}");
                    summary.skipped.push(reason);
                }
                ActionOutcome::Progressed => {}
                ActionOutcome::Completed => {
                    summary.terminal = Some(ReEvalTerminal::Completed);
                    return summary;
                }
                ActionOutcome::Halted { message } => {
                    summary.terminal = Some(ReEvalTerminal::Halted(message));
                    return summary;
                }
                ActionOutcome::Interrupted { message } => {
                    summary.terminal = Some(ReEvalTerminal::Interrupted(message));
                    return summary;
                }
            }
        }
    }

    summary
}
