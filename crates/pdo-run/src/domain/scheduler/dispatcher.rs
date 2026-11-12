//! The nodes a Run can spawn now, and the ones the session cap holds `waiting`.

use crate::domain::graph_resolver;
use crate::domain::state::NodeStatus;
use crate::domain::state::RunState;
use pdo_pipeline::domain::pipeline::PipelineDef;
use std::collections::HashMap;

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
            Some(n) => n.status.is_settled_complete(),
        })
        .map(|node_id| ReadySpawn { node_id, iter: 1 })
        .collect()
}

/// Drop from `ready` the nodes still held by a bounded loop's guarded exit,
/// read off every completed producer's frontmatter (ADR-0082). The projection
/// alone leaves a window open: a member settled on a FAIL lap whose routing has
/// not appended the next lap yet looks idle, so [`compute_ready_to_spawn`] reads
/// its exit as decided. Its own back-edge says the loop goes on: the node is
/// held, and the loop's routing starts it once the loop exits — PASS,
/// exhaustion, `end_region` or a `force_route` release it
/// ([`super::liveness::bounded_loop_still_iterating`]).
pub fn hold_pending_loop_exits(
    pipeline: &PipelineDef,
    run_state: &RunState,
    ready: Vec<ReadySpawn>,
    frontmatter_by_node: &HashMap<String, HashMap<String, serde_yaml::Value>>,
    vars: &HashMap<String, serde_yaml::Value>,
) -> Vec<ReadySpawn> {
    ready
        .into_iter()
        .filter(|r| {
            !loop_exit_pending_into(pipeline, run_state, &r.node_id, frontmatter_by_node, vars)
        })
        .collect()
}

/// Is a guarded edge into `node_id` fed by a settled bounded-loop member that
/// may still fire it on a later lap ([`super::liveness::loop_exit_pending`])?
/// Feedback edges (the producer downstream of the node) are no precondition.
fn loop_exit_pending_into(
    pipeline: &PipelineDef,
    run_state: &RunState,
    node_id: &str,
    frontmatter_by_node: &HashMap<String, HashMap<String, serde_yaml::Value>>,
    vars: &HashMap<String, serde_yaml::Value>,
) -> bool {
    pipeline
        .edges
        .iter()
        .filter(|e| e.target.node == node_id && (e.when.is_some() || e.is_else))
        .map(|e| e.source.node.as_str())
        .filter(|src| *src != node_id && !graph_resolver::reaches(pipeline, node_id, src))
        .filter(|src| {
            run_state
                .nodes
                .get(*src)
                .is_some_and(|n| n.status.is_settled_complete())
        })
        .any(|src| {
            super::liveness::loop_exit_pending(
                pipeline,
                run_state,
                src,
                node_id,
                frontmatter_by_node,
                vars,
            )
        })
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
    use crate::domain::state::NodeState;
    use pdo_pipeline::domain::pipeline::{
        EdgeDef, EdgeEndpoint, EdgeSource, NodeDef, NodeType, Port, PortType,
    };
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    fn make_node(id: &str, inputs: &[&str], outputs: &[&str]) -> NodeDef {
        NodeDef {
            free_live_artifacts: false,
            live_artifacts: Vec::new(),
            skills: Vec::new(),
            isolated_worktree: None,
            id: id.into(),
            name: id.into(),
            node_type: NodeType::Agent,
            inputs: inputs
                .iter()
                .map(|n| Port {
                    name: (*n).into(),
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                })
                .collect(),
            outputs: outputs
                .iter()
                .map(|n| Port {
                    name: (*n).into(),
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                })
                .collect(),
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
            pin_harness: None,
            harnesses: Default::default(),
            agent_choice: None,
            auto_fail: None,
            orchestrator: false,
            on_demand: false,
        }
    }

    fn make_edge(src_node: &str, src_port: &str, tgt_node: &str, tgt_port: &str) -> EdgeDef {
        EdgeDef {
            source: EdgeSource::single(src_node, src_port),
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
            missing_skills: Vec::new(),
            skipped_skills: Vec::new(),
            skills: None,
            isolated_worktree: None,
            harness: None,
            cost: None,
            node_id: id.into(),
            status: NodeStatus::Running,
            iter: 1,
            started_at: Some("t0".into()),
            completed_at: None,
            failure_reason: None,
            skip_reason: None,
            pruned: false,
            iterations: Vec::new(),
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
            missing_outputs: Vec::new(),
            delivery: None,
            awaiting: None,
        }
    }

    fn waiting_node(id: &str, iter: i64) -> NodeState {
        NodeState {
            missing_skills: Vec::new(),
            skipped_skills: Vec::new(),
            skills: None,
            isolated_worktree: None,
            harness: None,
            cost: None,
            node_id: id.into(),
            status: NodeStatus::Waiting,
            iter,
            started_at: None,
            completed_at: None,
            failure_reason: None,
            skip_reason: None,
            pruned: false,
            iterations: Vec::new(),
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
            missing_outputs: Vec::new(),
            delivery: None,
            awaiting: None,
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
            missing_skills: Vec::new(),
            skipped_skills: Vec::new(),
            skills: None,
            isolated_worktree: None,
            harness: None,
            cost: None,
            node_id: id.into(),
            status: NodeStatus::Completed,
            iter: 1,
            started_at: Some("t0".into()),
            completed_at: Some("t1".into()),
            failure_reason: None,
            skip_reason: None,
            pruned: false,
            iterations: Vec::new(),
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
            missing_outputs: Vec::new(),
            delivery: None,
            awaiting: None,
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
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
        };

        let state = empty_run_state();
        let ready = compute_ready_to_spawn(&pipeline, &state);
        assert_eq!(
            ready,
            vec![ReadySpawn {
                node_id: "planner".into(),
                iter: 1,
            }]
        );

        let mut state = empty_run_state();
        state
            .nodes
            .insert("planner".into(), running_node("planner"));
        let ready = compute_ready_to_spawn(&pipeline, &state);
        assert!(ready.is_empty(), "running node should not be re-spawned");

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

        state
            .nodes
            .insert("implementer".into(), running_node("implementer"));
        let ready = compute_ready_to_spawn(&pipeline, &state);
        assert!(ready.is_empty());
    }
}
