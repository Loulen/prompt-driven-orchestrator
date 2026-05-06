use std::collections::HashSet;

use crate::event_log::{NodeStatus, RunState};
use crate::pipeline::PipelineDef;

pub fn ready_nodes(pipeline: &PipelineDef, run_state: &RunState) -> Vec<String> {
    let mut ready = Vec::new();

    for node in &pipeline.nodes {
        // Skip nodes that already have a state (started, running, completed, failed)
        if run_state.nodes.contains_key(&node.id) {
            continue;
        }

        // Find all edges targeting this node
        let upstream_nodes: HashSet<&str> = pipeline
            .edges
            .iter()
            .filter(|e| e.target.node == node.id)
            .map(|e| e.source.node.as_str())
            .collect();

        if upstream_nodes.is_empty() {
            // Entry node — no dependencies, always ready
            ready.push(node.id.clone());
        } else {
            // Ready only if ALL upstream nodes are completed
            let all_completed = upstream_nodes.iter().all(|src| {
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
mod tests {
    use super::*;
    use crate::event_log::NodeState;
    use crate::pipeline::{EdgeDef, EdgeTarget, NodeDef, NodeType, Port};
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
            source: EdgeTarget {
                node: src_node.into(),
                port: src_port.into(),
            },
            target: EdgeTarget {
                node: tgt_node.into(),
                port: tgt_port.into(),
            },
            when: None,
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

    #[test]
    fn linear_chain_first_node_ready() {
        // Planner → Implementer → Reviewer
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
        // Planner → Impl-A, Planner → Impl-B
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
        // Impl-A → Merger, Impl-B → Merger
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

        // Only impl-a completed — merger not ready
        let mut state = empty_run_state();
        state
            .nodes
            .insert("impl-a".into(), completed_node("impl-a"));
        state.nodes.insert("impl-b".into(), running_node("impl-b"));
        let ready = ready_nodes(&pipeline, &state);
        assert!(ready.is_empty());

        // Both completed — merger ready
        state
            .nodes
            .insert("impl-b".into(), completed_node("impl-b"));
        let ready = ready_nodes(&pipeline, &state);
        assert_eq!(ready, vec!["merger"]);
    }

    #[test]
    fn partial_completion_next_ready_set() {
        // A → B → D, A → C → D
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

        // A completed, B running, C not started
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
}
