use std::collections::{HashMap, HashSet};

use crate::condition;
use crate::event_log::{NodeStatus, RunState};
use crate::loop_body_resolver;
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

pub fn ready_nodes(pipeline: &PipelineDef, run_state: &RunState) -> Vec<String> {
    let mut ready = Vec::new();

    for node in &pipeline.nodes {
        if node.node_type == NodeType::Start
            || node.node_type == NodeType::End
            || node.node_type == NodeType::Loop
            || node.node_type == NodeType::ForEach
        {
            continue;
        }
        if run_state.nodes.contains_key(&node.id) {
            continue;
        }

        let upstream: HashSet<&str> = pipeline
            .edges
            .iter()
            .filter(|e| e.target.node == node.id)
            .map(|e| e.source.node.as_str())
            .filter(|src| {
                !pipeline
                    .nodes
                    .iter()
                    .any(|n| n.id == *src && n.node_type == NodeType::Start)
            })
            .collect();

        if upstream.is_empty() {
            ready.push(node.id.clone());
        } else {
            let all_completed = upstream.iter().all(|src| {
                run_state
                    .nodes
                    .get(*src)
                    .is_some_and(|n| n.status == NodeStatus::Completed)
            });
            if all_completed {
                ready.push(node.id.clone());
            }
        }
    }

    ready
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

    for edge in &pipeline.edges {
        if edge.source.node != completed_node_id {
            continue;
        }

        if let Some(ref port) = matched_port {
            if edge.source.port != *port {
                continue;
            }
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
        } else {
            let target_node = pipeline.nodes.iter().find(|n| n.id == *target_id);
            let is_loop_target = target_node.is_some_and(|n| n.node_type == NodeType::Loop);
            let is_foreach_target = target_node.is_some_and(|n| n.node_type == NodeType::ForEach);

            if is_loop_target {
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
                let all_upstream_done =
                    check_all_upstream_completed(pipeline, run_state, target_id, completed_node_id);

                if all_upstream_done {
                    let next_iter = run_state
                        .nodes
                        .get(target_id.as_str())
                        .map(|n| n.iter + 1)
                        .unwrap_or(1);

                    actions.push(SchedulerAction::Spawn {
                        node_id: target_id.clone(),
                        iter: next_iter,
                    });
                }
            }
        }
    }

    actions
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

    let body_nodes = match loop_body_resolver::compute_body_subgraph(pipeline, loop_node_id) {
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

    if loop_state.break_received || current_iter >= max_iter {
        if !loop_state.break_received {
            actions.push(SchedulerAction::LoopMaxReached {
                loop_node_id: loop_node_id.to_string(),
                max_iter,
            });
        }

        actions.push(SchedulerAction::LoopDone {
            loop_node_id: loop_node_id.to_string(),
        });

        // Fire done port
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

fn handle_foreach_input(
    pipeline: &PipelineDef,
    run_state: &RunState,
    foreach_node_id: &str,
    target_port: &str,
    frontmatter_fields: &HashMap<String, serde_yaml::Value>,
) -> Vec<SchedulerAction> {
    let mut actions = Vec::new();

    if !pipeline.nodes.iter().any(|n| n.id == foreach_node_id) {
        return actions;
    }

    match target_port {
        "in" => {
            if run_state.foreach_states.contains_key(foreach_node_id) {
                return actions;
            }

            let items = frontmatter_fields
                .get("items")
                .and_then(|v| v.as_sequence())
                .cloned()
                .unwrap_or_default();

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

    let body_nodes = match loop_body_resolver::compute_body_subgraph(pipeline, foreach_node_id) {
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
) -> bool {
    let upstream: HashSet<&str> = pipeline
        .edges
        .iter()
        .filter(|e| e.target.node == target_node_id)
        .map(|e| e.source.node.as_str())
        .collect();

    upstream.iter().all(|src| {
        if *src == just_completed_node_id {
            return true;
        }
        run_state
            .nodes
            .get(*src)
            .is_some_and(|n| n.status == NodeStatus::Completed)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_log::NodeState;
    use crate::pipeline::{EdgeDef, EdgeEndpoint, NodeDef, NodeType, Port};
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
                    frontmatter: None,
                    when: None,
                })
                .collect(),
            outputs: outputs
                .iter()
                .map(|n| Port {
                    name: (*n).into(),
                    repeated: false,
                    side: None,
                    frontmatter: None,
                    when: None,
                })
                .collect(),
            interactive: false,
            view: None,
            max_iter: None,
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
                frontmatter: None,
                when: None,
            }],
            outputs: vec![],
            interactive: false,
            view: None,
            max_iter: None,
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
            }],
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
        // reviewer completes at iter 2 → back-edge fires →
        // implementer already at iter 2, so next spawn is iter 3
        let pipeline = PipelineDef {
            name: "cycle".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("implementer", &["review"], &["code"]),
                make_node("reviewer", &["code"], &["review"]),
            ],
            edges: vec![make_edge("reviewer", "review", "implementer", "review")],
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
                frontmatter: None,
                when: None,
            }],
            outputs: branch_outputs,
            interactive: false,
            view: None,
            max_iter: None,
        }
    }

    fn switch_port(name: &str, when_yaml: &str) -> Port {
        Port {
            name: name.into(),
            repeated: false,
            side: None,
            frontmatter: None,
            when: Some(serde_yaml::from_str(when_yaml).unwrap()),
        }
    }

    fn switch_default_port() -> Port {
        Port {
            name: "default".into(),
            repeated: false,
            side: None,
            frontmatter: None,
            when: None,
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
                    frontmatter: None,
                    when: None,
                },
                Port {
                    name: "break".into(),
                    repeated: false,
                    side: None,
                    frontmatter: None,
                    when: None,
                },
            ],
            outputs: vec![
                Port {
                    name: "body".into(),
                    repeated: false,
                    side: None,
                    frontmatter: None,
                    when: None,
                },
                Port {
                    name: "done".into(),
                    repeated: false,
                    side: None,
                    frontmatter: None,
                    when: None,
                },
            ],
            interactive: false,
            view: None,
            max_iter: Some(serde_yaml::Value::Number(serde_yaml::Number::from(
                max_iter,
            ))),
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
                frontmatter: None,
                when: None,
            }],
            interactive: false,
            view: None,
            max_iter: None,
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
                    frontmatter: None,
                    when: None,
                },
                Port {
                    name: "break".into(),
                    repeated: false,
                    side: None,
                    frontmatter: None,
                    when: None,
                },
            ],
            outputs: vec![
                Port {
                    name: "body".into(),
                    repeated: false,
                    side: None,
                    frontmatter: None,
                    when: None,
                },
                Port {
                    name: "done".into(),
                    repeated: false,
                    side: None,
                    frontmatter: None,
                    when: None,
                },
            ],
            interactive: false,
            view: None,
            max_iter: None,
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
        };

        let state = empty_run_state();
        let ready = ready_nodes(&pipeline, &state);
        assert!(
            !ready.contains(&"fe1".to_string()),
            "ForEach should not appear in ready_nodes"
        );
    }
}
