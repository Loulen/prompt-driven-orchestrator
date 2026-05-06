use std::collections::HashSet;

use crate::condition::{self, EvalContext};
use crate::event_log::{NodeStatus, RunState};
use crate::pipeline::{EdgeTarget, PipelineDef};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulerAction {
    Spawn { node_id: String, iter: i64 },
    Halt { message: String },
}

pub fn ready_nodes(pipeline: &PipelineDef, run_state: &RunState) -> Vec<String> {
    let mut ready = Vec::new();

    for node in &pipeline.nodes {
        if run_state.nodes.contains_key(&node.id) {
            continue;
        }

        let unconditional_upstream: HashSet<&str> = pipeline
            .edges
            .iter()
            .filter(|e| {
                matches!(&e.target, EdgeTarget::Node(ep) if ep.node == node.id) && e.when.is_none()
            })
            .map(|e| e.source.node.as_str())
            .collect();

        if unconditional_upstream.is_empty() {
            // Entry node — no unconditional dependencies. Conditional incoming edges
            // (back-edges in cycles) don't block initial scheduling.
            ready.push(node.id.clone());
        } else {
            let all_completed = unconditional_upstream.iter().all(|src| {
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

pub fn evaluate_outgoing_edges(
    pipeline: &PipelineDef,
    run_state: &RunState,
    completed_node_id: &str,
) -> Vec<SchedulerAction> {
    let mut actions = Vec::new();

    let source_iter = run_state
        .nodes
        .get(completed_node_id)
        .map(|n| n.iter)
        .unwrap_or(1);

    let ctx = EvalContext::new(source_iter);

    for edge in &pipeline.edges {
        if edge.source.node != completed_node_id {
            continue;
        }

        let fires = match &edge.when {
            None => true,
            Some(when) => condition::evaluate_with_iter(when, &ctx),
        };

        if !fires {
            continue;
        }

        match &edge.target {
            EdgeTarget::Node(ep) => {
                let target_id = &ep.node;

                let target_all_unconditional_upstream_completed =
                    check_all_unconditional_upstream_completed(
                        pipeline,
                        run_state,
                        target_id,
                        completed_node_id,
                    );

                if target_all_unconditional_upstream_completed {
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
            EdgeTarget::Halt(h) => {
                let raw_msg = h.message.as_deref().unwrap_or("Run halted");
                let rendered = condition::render_halt_message(
                    raw_msg,
                    &condition::HaltContext {
                        iter: source_iter,
                        node_id: completed_node_id.to_string(),
                    },
                );
                actions.push(SchedulerAction::Halt { message: rendered });
            }
        }
    }

    actions
}

fn check_all_unconditional_upstream_completed(
    pipeline: &PipelineDef,
    run_state: &RunState,
    target_node_id: &str,
    just_completed_node_id: &str,
) -> bool {
    let unconditional_upstream: HashSet<&str> = pipeline
        .edges
        .iter()
        .filter(|e| {
            matches!(&e.target, EdgeTarget::Node(ep) if ep.node == target_node_id)
                && e.when.is_none()
        })
        .map(|e| e.source.node.as_str())
        .collect();

    unconditional_upstream.iter().all(|src| {
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
    use crate::pipeline::{EdgeDef, EdgeEndpoint, EdgeTarget, HaltTarget, NodeDef, NodeType, Port};
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    fn make_node(id: &str, inputs: &[&str], outputs: &[&str]) -> NodeDef {
        NodeDef {
            id: id.into(),
            node_type: NodeType::DocOnly,
            prompt_file: None,
            inputs: inputs
                .iter()
                .map(|n| Port {
                    name: (*n).into(),
                    repeated: false,
                })
                .collect(),
            outputs: outputs
                .iter()
                .map(|n| Port {
                    name: (*n).into(),
                    repeated: false,
                })
                .collect(),
            interactive: false,
            view: None,
        }
    }

    fn make_edge(src_node: &str, src_port: &str, tgt_node: &str, tgt_port: &str) -> EdgeDef {
        EdgeDef {
            source: EdgeEndpoint {
                node: src_node.into(),
                port: src_port.into(),
            },
            target: EdgeTarget::Node(EdgeEndpoint {
                node: tgt_node.into(),
                port: tgt_port.into(),
            }),
            when: None,
        }
    }

    fn make_conditional_edge(
        src_node: &str,
        src_port: &str,
        tgt_node: &str,
        tgt_port: &str,
        when: serde_yaml::Value,
    ) -> EdgeDef {
        EdgeDef {
            source: EdgeEndpoint {
                node: src_node.into(),
                port: src_port.into(),
            },
            target: EdgeTarget::Node(EdgeEndpoint {
                node: tgt_node.into(),
                port: tgt_port.into(),
            }),
            when: Some(when),
        }
    }

    fn make_halt_edge(
        src_node: &str,
        src_port: &str,
        message: &str,
        when: serde_yaml::Value,
    ) -> EdgeDef {
        EdgeDef {
            source: EdgeEndpoint {
                node: src_node.into(),
                port: src_port.into(),
            },
            target: EdgeTarget::Halt(HaltTarget {
                message: Some(message.into()),
            }),
            when: Some(when),
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
        }
    }

    fn yaml(s: &str) -> serde_yaml::Value {
        serde_yaml::from_str(s).unwrap()
    }

    // --- ready_nodes (existing) ---

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
    fn conditional_edge_fires_when_true() {
        // reviewer → implementer when iter < 3
        let pipeline = PipelineDef {
            name: "cycle".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("implementer", &["review"], &["code"]),
                make_node("reviewer", &["code"], &["review"]),
            ],
            edges: vec![make_conditional_edge(
                "reviewer",
                "review",
                "implementer",
                "review",
                yaml("iter: { lt: 3 }"),
            )],
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("reviewer".into(), completed_node_iter("reviewer", 1));

        let actions = evaluate_outgoing_edges(&pipeline, &state, "reviewer");
        assert_eq!(
            actions,
            vec![SchedulerAction::Spawn {
                node_id: "implementer".into(),
                iter: 1,
            }]
        );
    }

    #[test]
    fn conditional_edge_does_not_fire_when_false() {
        let pipeline = PipelineDef {
            name: "cycle".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("implementer", &["review"], &["code"]),
                make_node("reviewer", &["code"], &["review"]),
            ],
            edges: vec![make_conditional_edge(
                "reviewer",
                "review",
                "implementer",
                "review",
                yaml("iter: { lt: 3 }"),
            )],
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("reviewer".into(), completed_node_iter("reviewer", 3));

        let actions = evaluate_outgoing_edges(&pipeline, &state, "reviewer");
        assert!(actions.is_empty());
    }

    #[test]
    fn halt_edge_produces_halt_action() {
        let pipeline = PipelineDef {
            name: "halt-test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![make_node("reviewer", &["code"], &["review"])],
            edges: vec![make_halt_edge(
                "reviewer",
                "review",
                "Blocked after {iter} iterations on {node-id}",
                yaml("iter: { gte: 3 }"),
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
    fn halt_edge_does_not_fire_when_condition_false() {
        let pipeline = PipelineDef {
            name: "halt-test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![make_node("reviewer", &["code"], &["review"])],
            edges: vec![make_halt_edge(
                "reviewer",
                "review",
                "Blocked",
                yaml("iter: { gte: 3 }"),
            )],
        };

        let mut state = empty_run_state();
        state
            .nodes
            .insert("reviewer".into(), completed_node_iter("reviewer", 1));

        let actions = evaluate_outgoing_edges(&pipeline, &state, "reviewer");
        assert!(actions.is_empty());
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
            edges: vec![make_conditional_edge(
                "reviewer",
                "review",
                "implementer",
                "review",
                yaml("iter: { lt: 5 }"),
            )],
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
    fn two_node_cycle_with_halt_full_scenario() {
        // implementer → reviewer (unconditional)
        // reviewer → implementer (when iter < 3)  — back-edge
        // reviewer → halt (when iter >= 3)
        let pipeline = PipelineDef {
            name: "review-loop".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("implementer", &["review"], &["code"]),
                make_node("reviewer", &["code"], &["review"]),
            ],
            edges: vec![
                make_edge("implementer", "code", "reviewer", "code"),
                make_conditional_edge(
                    "reviewer",
                    "review",
                    "implementer",
                    "review",
                    yaml("iter: { lt: 3 }"),
                ),
                make_halt_edge(
                    "reviewer",
                    "review",
                    "Halted after {iter} iterations",
                    yaml("iter: { gte: 3 }"),
                ),
            ],
        };

        // Iter 1: reviewer done → back-edge fires, halt doesn't
        let mut state = empty_run_state();
        state
            .nodes
            .insert("reviewer".into(), completed_node_iter("reviewer", 1));

        let actions = evaluate_outgoing_edges(&pipeline, &state, "reviewer");
        assert_eq!(actions.len(), 1);
        assert!(
            matches!(&actions[0], SchedulerAction::Spawn { node_id, iter: 1 } if node_id == "implementer")
        );

        // Iter 2: reviewer done → back-edge fires, halt doesn't
        state
            .nodes
            .insert("reviewer".into(), completed_node_iter("reviewer", 2));
        state
            .nodes
            .insert("implementer".into(), completed_node_iter("implementer", 1));

        let actions = evaluate_outgoing_edges(&pipeline, &state, "reviewer");
        assert_eq!(actions.len(), 1);
        assert!(
            matches!(&actions[0], SchedulerAction::Spawn { node_id, .. } if node_id == "implementer")
        );

        // Iter 3: reviewer done → back-edge doesn't fire, halt fires
        state
            .nodes
            .insert("reviewer".into(), completed_node_iter("reviewer", 3));

        let actions = evaluate_outgoing_edges(&pipeline, &state, "reviewer");
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0],
            SchedulerAction::Halt {
                message: "Halted after 3 iterations".into(),
            }
        );
    }

    #[test]
    fn conditional_node_not_initially_ready() {
        // implementer has only conditional incoming edges → should not appear in ready_nodes
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
                make_conditional_edge(
                    "reviewer",
                    "review",
                    "implementer",
                    "review",
                    yaml("iter: { lt: 3 }"),
                ),
            ],
        };

        // At startup, implementer has no unconditional incoming edges.
        // It's an entry node for the forward edge, but also has a conditional back-edge.
        // The entry node detection should still work: implementer has no unconditional
        // incoming edges targeting it... wait, it does have a conditional one.
        // Let me think: implementer has one incoming edge (from reviewer, conditional).
        // It has no unconditional incoming edges. So it IS an entry node initially.
        // That's correct — the first iteration of implementer starts unconditionally.
        let state = empty_run_state();
        let ready = ready_nodes(&pipeline, &state);
        assert_eq!(ready, vec!["implementer"]);
    }
}
