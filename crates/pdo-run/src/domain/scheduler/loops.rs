//! The legacy `loop` node: its iteration cap, its input, the completion of its body.

use super::*;

pub fn resolve_max_iter(
    loop_node: &pdo_pipeline::domain::pipeline::NodeDef,
    resolved_vars: &HashMap<String, serde_yaml::Value>,
) -> i64 {
    pdo_pipeline::domain::loops::resolve_node_max_iter(loop_node, resolved_vars)
}

pub(super) fn handle_loop_input(
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

            for edge in &pipeline.edges {
                if edge.source.node == loop_node_id && edge.source.carries("body") {
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

    // Break is unconditional termination — skip the body completion check.
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
            .is_some_and(|n| n.status.is_settled_complete() && n.iter >= current_iter)
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

        for edge in &pipeline.edges {
            if edge.source.node == loop_node_id && edge.source.carries("body") {
                actions.push(SchedulerAction::Spawn {
                    node_id: edge.target.node.clone(),
                    iter: next_iter,
                });
            }
        }
    }

    actions
}

pub(super) fn fire_done_port(
    pipeline: &PipelineDef,
    loop_node_id: &str,
    actions: &mut Vec<SchedulerAction>,
) {
    for edge in &pipeline.edges {
        if edge.source.node == loop_node_id && edge.source.carries("done") {
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
