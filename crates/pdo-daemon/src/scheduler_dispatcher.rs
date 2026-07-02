use crate::event_log::{NodeStatus, RunState};
use crate::graph_resolver;
use crate::pipeline::PipelineDef;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadySpawn {
    pub node_id: String,
    pub iter: i64,
}

pub fn compute_ready_to_spawn(pipeline: &PipelineDef, run_state: &RunState) -> Vec<ReadySpawn> {
    graph_resolver::ready_nodes(pipeline, run_state)
        .into_iter()
        .filter(|node_id| match run_state.nodes.get(node_id) {
            None => true,
            Some(n) => n.status == NodeStatus::Completed,
        })
        .map(|node_id| ReadySpawn { node_id, iter: 1 })
        .collect()
}

/// Nodes currently throttled into the `Waiting` state for this run.
///
/// These already exist in the run state (so [`compute_ready_to_spawn`] skips
/// them) but hold no session yet. The dispatcher retries them against the
/// session cap whenever a slot may have freed (admission control, #159).
pub fn waiting_nodes(run_state: &RunState) -> Vec<ReadySpawn> {
    let mut waiting: Vec<ReadySpawn> = run_state
        .nodes
        .values()
        .filter(|n| n.status == NodeStatus::Waiting)
        .map(|n| ReadySpawn {
            node_id: n.node_id.clone(),
            iter: n.iter,
        })
        .collect();
    // Deterministic order so retries are reproducible regardless of HashMap
    // iteration order.
    waiting.sort_by(|a, b| a.node_id.cmp(&b.node_id).then(a.iter.cmp(&b.iter)));
    waiting
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_log::NodeState;
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
            model: None,
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

    fn empty_run_state() -> RunState {
        RunState::new("run-1".into(), "test".into())
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

    fn waiting_node(id: &str, iter: i64) -> NodeState {
        NodeState {
            node_id: id.into(),
            status: NodeStatus::Waiting,
            iter,
            started_at: None,
            completed_at: None,
            failure_reason: None,
            iterations: Vec::new(),
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
        }
    }

    #[test]
    fn waiting_nodes_returns_throttled_nodes_in_deterministic_order() {
        let mut state = empty_run_state();
        state.nodes.insert("zed".into(), waiting_node("zed", 2));
        state.nodes.insert("abe".into(), waiting_node("abe", 1));
        state.nodes.insert("runner".into(), running_node("runner"));
        state.nodes.insert("done".into(), completed_node("done"));

        let waiting = waiting_nodes(&state);
        assert_eq!(
            waiting,
            vec![
                ReadySpawn {
                    node_id: "abe".into(),
                    iter: 1,
                },
                ReadySpawn {
                    node_id: "zed".into(),
                    iter: 2,
                },
            ],
            "only Waiting nodes, sorted, with their preserved iter"
        );
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

    #[test]
    fn idempotent_no_double_spawn() {
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

        // First call: planner is ready (entry node, no state yet)
        let state = empty_run_state();
        let ready = compute_ready_to_spawn(&pipeline, &state);
        assert_eq!(
            ready,
            vec![ReadySpawn {
                node_id: "planner".into(),
                iter: 1,
            }]
        );

        // Second call: planner is now Running (simulates after first spawn)
        let mut state = empty_run_state();
        state
            .nodes
            .insert("planner".into(), running_node("planner"));
        let ready = compute_ready_to_spawn(&pipeline, &state);
        assert!(ready.is_empty(), "running node should not be re-spawned");

        // Third call: planner completed, implementer becomes ready
        let mut state = empty_run_state();
        state
            .nodes
            .insert("planner".into(), completed_node("planner"));
        let ready = compute_ready_to_spawn(&pipeline, &state);
        assert_eq!(
            ready,
            vec![ReadySpawn {
                node_id: "implementer".into(),
                iter: 1,
            }]
        );

        // Fourth call: implementer now running too — nothing to spawn
        state
            .nodes
            .insert("implementer".into(), running_node("implementer"));
        let ready = compute_ready_to_spawn(&pipeline, &state);
        assert!(ready.is_empty());
    }
}
