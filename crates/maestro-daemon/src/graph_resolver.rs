use std::collections::HashSet;

use crate::event_log::{NodeStatus, RunState};
use crate::pipeline::{NodeType, PipelineDef};

/// Returns the IDs of nodes that are ready to be spawned: all upstream
/// dependencies completed, node not yet started, and not a control-flow
/// construct (Start, End, Loop, ForEach).
pub fn ready_nodes(pipeline: &PipelineDef, run_state: &RunState) -> Vec<String> {
    let mut ready = Vec::new();

    for node in &pipeline.nodes {
        if matches!(
            node.node_type,
            NodeType::Start | NodeType::End | NodeType::Loop | NodeType::ForEach | NodeType::Switch
        ) {
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

    ready
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BodyResolutionError {
    #[error("loop node '{0}' not found in pipeline")]
    LoopNotFound(String),
    #[error("loop '{0}' has an empty body (body port wired to nothing)")]
    EmptyBody(String),
    #[error("loop '{0}' body has no exit path back to break or done")]
    NoExitToBreakOrDone(String),
}

/// Computes the set of nodes that form the body subgraph of a Loop or ForEach
/// node. BFS from the loop's "body" output port, collecting all reachable
/// nodes until hitting the loop's own break/done ports. Nested loops are
/// treated as opaque (included but their internals are not traversed).
pub fn compute_body_subgraph(
    pipeline: &PipelineDef,
    loop_node_id: &str,
) -> Result<HashSet<String>, BodyResolutionError> {
    pipeline
        .nodes
        .iter()
        .find(|n| n.id == loop_node_id && matches!(n.node_type, NodeType::Loop | NodeType::ForEach))
        .ok_or_else(|| BodyResolutionError::LoopNotFound(loop_node_id.to_string()))?;

    let body_targets: Vec<&str> = pipeline
        .edges
        .iter()
        .filter(|e| e.source.node == loop_node_id && e.source.port == "body")
        .map(|e| e.target.node.as_str())
        .collect();

    if body_targets.is_empty() {
        return Err(BodyResolutionError::EmptyBody(loop_node_id.to_string()));
    }

    let mut body = HashSet::new();
    let mut queue: Vec<&str> = body_targets;

    while let Some(current) = queue.pop() {
        if current == loop_node_id {
            continue;
        }

        let is_nested_loop = pipeline
            .nodes
            .iter()
            .any(|n| n.id == current && matches!(n.node_type, NodeType::Loop | NodeType::ForEach));
        if is_nested_loop {
            body.insert(current.to_string());
            continue;
        }

        if !body.insert(current.to_string()) {
            continue;
        }

        for edge in &pipeline.edges {
            if edge.source.node != current {
                continue;
            }
            let target = edge.target.node.as_str();
            if target == loop_node_id {
                continue;
            }
            if !body.contains(target) {
                queue.push(target);
            }
        }
    }

    let has_exit = pipeline.edges.iter().any(|e| {
        body.contains(&e.source.node)
            && e.target.node == loop_node_id
            && (e.target.port == "break" || e.target.port == "done")
    });

    if !has_exit {
        return Err(BodyResolutionError::NoExitToBreakOrDone(
            loop_node_id.to_string(),
        ));
    }

    Ok(body)
}

/// Returns the set of all nodes transitively reachable from `node_id` by
/// following outgoing edges. The starting node is NOT included in the result.
pub fn downstream_subgraph(pipeline: &PipelineDef, node_id: &str) -> HashSet<String> {
    let mut visited = HashSet::new();
    visited.insert(node_id.to_string());
    let mut queue = vec![node_id.to_string()];

    while let Some(current) = queue.pop() {
        for edge in &pipeline.edges {
            if edge.source.node == current {
                let target = &edge.target.node;
                if visited.insert(target.clone()) {
                    queue.push(target.clone());
                }
            }
        }
    }

    visited.remove(node_id);
    visited
}

/// Returns the number of pipeline nodes that are not yet completed.
/// Excludes Start and End control-flow nodes from the count.
pub fn nodes_remaining(pipeline: &PipelineDef, run_state: &RunState) -> usize {
    pipeline
        .nodes
        .iter()
        .filter(|n| !matches!(n.node_type, NodeType::Start | NodeType::End))
        .filter(|n| {
            !run_state
                .nodes
                .get(&n.id)
                .is_some_and(|ns| ns.status == NodeStatus::Completed)
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_log::NodeState;
    use crate::pipeline::{EdgeDef, EdgeEndpoint, NodeDef, NodeType, Port};
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    fn make_node(id: &str, node_type: NodeType, inputs: &[&str], outputs: &[&str]) -> NodeDef {
        NodeDef {
            id: id.into(),
            name: id.into(),
            node_type,
            inputs: inputs
                .iter()
                .map(|n| Port {
                    name: (*n).into(),
                    repeated: false,
                    side: None,
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
                    description: None,
                },
                Port {
                    name: "break".into(),
                    repeated: false,
                    side: None,
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
                    frontmatter: None,
                    when: None,
                    description: None,
                },
                Port {
                    name: "done".into(),
                    repeated: false,
                    side: None,
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

    fn make_pipeline(nodes: Vec<NodeDef>, edges: Vec<EdgeDef>) -> PipelineDef {
        PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes,
            edges,
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

    // ========== ready_nodes ==========

    #[test]
    fn ready_nodes_skips_switch() {
        let pipeline = make_pipeline(
            vec![
                make_node("upstream", NodeType::DocOnly, &["in"], &["out"]),
                make_node("sw", NodeType::Switch, &["in"], &["pass", "default"]),
                make_node("downstream", NodeType::DocOnly, &["in"], &["out"]),
            ],
            vec![
                make_edge("upstream", "out", "sw", "in"),
                make_edge("sw", "pass", "downstream", "in"),
            ],
        );

        let mut state = empty_run_state();
        state
            .nodes
            .insert("upstream".into(), completed_node("upstream"));
        let ready = ready_nodes(&pipeline, &state);
        assert!(
            !ready.contains(&"sw".to_string()),
            "Switch nodes must never appear in ready_nodes"
        );
    }

    #[test]
    fn ready_nodes_linear_chain_first_ready() {
        let pipeline = make_pipeline(
            vec![
                make_node("planner", NodeType::DocOnly, &["task"], &["plan"]),
                make_node("implementer", NodeType::DocOnly, &["plan"], &["summary"]),
            ],
            vec![make_edge("planner", "plan", "implementer", "plan")],
        );

        let state = empty_run_state();
        let ready = ready_nodes(&pipeline, &state);
        assert_eq!(ready, vec!["planner"]);
    }

    #[test]
    fn ready_nodes_fan_in_waits_for_all() {
        let pipeline = make_pipeline(
            vec![
                make_node("a", NodeType::DocOnly, &["task"], &["out"]),
                make_node("b", NodeType::DocOnly, &["task"], &["out"]),
                make_node("c", NodeType::DocOnly, &["in-a", "in-b"], &["out"]),
            ],
            vec![
                make_edge("a", "out", "c", "in-a"),
                make_edge("b", "out", "c", "in-b"),
            ],
        );

        let mut state = empty_run_state();
        state.nodes.insert("a".into(), completed_node("a"));
        state.nodes.insert("b".into(), running_node("b"));
        assert!(ready_nodes(&pipeline, &state).is_empty());

        state.nodes.insert("b".into(), completed_node("b"));
        assert_eq!(ready_nodes(&pipeline, &state), vec!["c"]);
    }

    #[test]
    fn ready_nodes_skips_loop_and_foreach() {
        let pipeline = make_pipeline(
            vec![
                make_loop_node("loop1", 5),
                make_node("worker", NodeType::DocOnly, &["in"], &["out"]),
            ],
            vec![make_edge("loop1", "body", "worker", "in")],
        );

        let ready = ready_nodes(&pipeline, &empty_run_state());
        assert!(!ready.contains(&"loop1".to_string()));
    }

    // ========== compute_body_subgraph ==========

    #[test]
    fn body_subgraph_linear_body() {
        let pipeline = make_pipeline(
            vec![
                make_loop_node("loop1", 5),
                make_node("a", NodeType::DocOnly, &["in"], &["out"]),
                make_node("b", NodeType::DocOnly, &["in"], &["out"]),
                make_node("sw", NodeType::Switch, &["in"], &["pass", "default"]),
            ],
            vec![
                make_edge("loop1", "body", "a", "in"),
                make_edge("a", "out", "b", "in"),
                make_edge("b", "out", "sw", "in"),
                make_edge("sw", "pass", "loop1", "break"),
            ],
        );

        let body = compute_body_subgraph(&pipeline, "loop1").unwrap();
        let expected: HashSet<String> = ["a", "b", "sw"].iter().map(|s| s.to_string()).collect();
        assert_eq!(body, expected);
    }

    #[test]
    fn body_subgraph_nested_loops_opaque() {
        let pipeline = make_pipeline(
            vec![
                make_loop_node("outer", 3),
                make_loop_node("inner", 5),
                make_node("inner_worker", NodeType::DocOnly, &["in"], &["out"]),
            ],
            vec![
                make_edge("outer", "body", "inner", "in"),
                make_edge("inner", "body", "inner_worker", "in"),
                make_edge("inner_worker", "out", "inner", "break"),
                make_edge("inner", "done", "outer", "break"),
            ],
        );

        let body = compute_body_subgraph(&pipeline, "outer").unwrap();
        let expected: HashSet<String> = ["inner"].iter().map(|s| s.to_string()).collect();
        assert_eq!(body, expected);
    }

    #[test]
    fn body_subgraph_empty_body_error() {
        let pipeline = make_pipeline(vec![make_loop_node("loop1", 5)], vec![]);
        assert_eq!(
            compute_body_subgraph(&pipeline, "loop1"),
            Err(BodyResolutionError::EmptyBody("loop1".into()))
        );
    }

    #[test]
    fn body_subgraph_loop_not_found() {
        let pipeline = make_pipeline(vec![], vec![]);
        assert_eq!(
            compute_body_subgraph(&pipeline, "nonexistent"),
            Err(BodyResolutionError::LoopNotFound("nonexistent".into()))
        );
    }

    #[test]
    fn body_subgraph_non_loop_node_returns_error() {
        let pipeline = make_pipeline(
            vec![make_node("a", NodeType::DocOnly, &["in"], &["out"])],
            vec![],
        );
        assert_eq!(
            compute_body_subgraph(&pipeline, "a"),
            Err(BodyResolutionError::LoopNotFound("a".into()))
        );
    }

    #[test]
    fn body_subgraph_no_exit_returns_error() {
        let pipeline = make_pipeline(
            vec![
                make_loop_node("loop1", 5),
                make_node("a", NodeType::DocOnly, &["in"], &["out"]),
                make_node("b", NodeType::DocOnly, &["in"], &["out"]),
            ],
            vec![
                make_edge("loop1", "body", "a", "in"),
                make_edge("a", "out", "b", "in"),
            ],
        );

        assert_eq!(
            compute_body_subgraph(&pipeline, "loop1"),
            Err(BodyResolutionError::NoExitToBreakOrDone("loop1".into()))
        );
    }

    #[test]
    fn body_subgraph_internal_switch_all_branches_stay() {
        let pipeline = make_pipeline(
            vec![
                make_loop_node("loop1", 5),
                make_node("impl", NodeType::CodeMutating, &["in"], &["out"]),
                make_node("reviewer", NodeType::DocOnly, &["in"], &["review"]),
                make_node("sw", NodeType::Switch, &["in"], &["pass", "default"]),
            ],
            vec![
                make_edge("loop1", "body", "impl", "in"),
                make_edge("impl", "out", "reviewer", "in"),
                make_edge("reviewer", "review", "sw", "in"),
                make_edge("sw", "pass", "loop1", "break"),
                make_edge("sw", "default", "impl", "in"),
            ],
        );

        let body = compute_body_subgraph(&pipeline, "loop1").unwrap();
        let expected: HashSet<String> = ["impl", "reviewer", "sw"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(body, expected);
    }

    // ========== downstream_subgraph ==========

    #[test]
    fn downstream_linear_chain() {
        let pipeline = make_pipeline(
            vec![
                make_node("a", NodeType::DocOnly, &["in"], &["out"]),
                make_node("b", NodeType::DocOnly, &["in"], &["out"]),
                make_node("c", NodeType::DocOnly, &["in"], &["out"]),
            ],
            vec![
                make_edge("a", "out", "b", "in"),
                make_edge("b", "out", "c", "in"),
            ],
        );

        let ds = downstream_subgraph(&pipeline, "a");
        let expected: HashSet<String> = ["b", "c"].iter().map(|s| s.to_string()).collect();
        assert_eq!(ds, expected);
    }

    #[test]
    fn downstream_branching_dag() {
        let pipeline = make_pipeline(
            vec![
                make_node("a", NodeType::DocOnly, &["in"], &["out"]),
                make_node("b", NodeType::DocOnly, &["in"], &["out"]),
                make_node("c", NodeType::DocOnly, &["in"], &["out"]),
                make_node("d", NodeType::DocOnly, &["in-b", "in-c"], &["out"]),
            ],
            vec![
                make_edge("a", "out", "b", "in"),
                make_edge("a", "out", "c", "in"),
                make_edge("b", "out", "d", "in-b"),
                make_edge("c", "out", "d", "in-c"),
            ],
        );

        let ds = downstream_subgraph(&pipeline, "a");
        let expected: HashSet<String> = ["b", "c", "d"].iter().map(|s| s.to_string()).collect();
        assert_eq!(ds, expected);
    }

    #[test]
    fn downstream_from_leaf_is_empty() {
        let pipeline = make_pipeline(
            vec![
                make_node("a", NodeType::DocOnly, &["in"], &["out"]),
                make_node("b", NodeType::DocOnly, &["in"], &["out"]),
            ],
            vec![make_edge("a", "out", "b", "in")],
        );

        let ds = downstream_subgraph(&pipeline, "b");
        assert!(ds.is_empty());
    }

    #[test]
    fn downstream_loop_body() {
        let pipeline = make_pipeline(
            vec![
                make_loop_node("loop1", 5),
                make_node("impl", NodeType::CodeMutating, &["in"], &["out"]),
                make_node("reviewer", NodeType::DocOnly, &["in"], &["review"]),
                make_node("downstream", NodeType::DocOnly, &["in"], &["out"]),
            ],
            vec![
                make_edge("loop1", "body", "impl", "in"),
                make_edge("impl", "out", "reviewer", "in"),
                make_edge("reviewer", "review", "loop1", "break"),
                make_edge("loop1", "done", "downstream", "in"),
            ],
        );

        let ds = downstream_subgraph(&pipeline, "loop1");
        let expected: HashSet<String> = ["impl", "reviewer", "downstream"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(ds, expected);
    }

    #[test]
    fn downstream_nested_loops() {
        let pipeline = make_pipeline(
            vec![
                make_loop_node("outer", 3),
                make_loop_node("inner", 5),
                make_node("worker", NodeType::DocOnly, &["in"], &["out"]),
                make_node("final", NodeType::DocOnly, &["in"], &["out"]),
            ],
            vec![
                make_edge("outer", "body", "inner", "in"),
                make_edge("inner", "body", "worker", "in"),
                make_edge("worker", "out", "inner", "break"),
                make_edge("inner", "done", "outer", "break"),
                make_edge("outer", "done", "final", "in"),
            ],
        );

        let ds = downstream_subgraph(&pipeline, "outer");
        let expected: HashSet<String> = ["inner", "worker", "final"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(ds, expected);
    }

    #[test]
    fn downstream_switch_routing() {
        let pipeline = make_pipeline(
            vec![
                make_node(
                    "sw",
                    NodeType::Switch,
                    &["in"],
                    &["pass", "fail", "default"],
                ),
                make_node("pass-handler", NodeType::DocOnly, &["in"], &["out"]),
                make_node("fail-handler", NodeType::DocOnly, &["in"], &["out"]),
                make_node("merge", NodeType::DocOnly, &["in-p", "in-f"], &["out"]),
            ],
            vec![
                make_edge("sw", "pass", "pass-handler", "in"),
                make_edge("sw", "fail", "fail-handler", "in"),
                make_edge("pass-handler", "out", "merge", "in-p"),
                make_edge("fail-handler", "out", "merge", "in-f"),
            ],
        );

        let ds = downstream_subgraph(&pipeline, "sw");
        let expected: HashSet<String> = ["pass-handler", "fail-handler", "merge"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(ds, expected);
    }

    #[test]
    fn downstream_does_not_include_start_node() {
        let pipeline = make_pipeline(
            vec![
                make_node("a", NodeType::DocOnly, &["in"], &["out"]),
                make_node("b", NodeType::DocOnly, &["in"], &["out"]),
            ],
            vec![
                make_edge("a", "out", "b", "in"),
                make_edge("b", "out", "a", "in"),
            ],
        );

        let ds = downstream_subgraph(&pipeline, "a");
        assert!(ds.contains("b"));
        assert!(!ds.contains("a"));
    }

    // ========== nodes_remaining ==========

    #[test]
    fn nodes_remaining_all_pending() {
        let pipeline = make_pipeline(
            vec![
                make_node("a", NodeType::DocOnly, &["in"], &["out"]),
                make_node("b", NodeType::DocOnly, &["in"], &["out"]),
                make_node("c", NodeType::DocOnly, &["in"], &["out"]),
            ],
            vec![
                make_edge("a", "out", "b", "in"),
                make_edge("b", "out", "c", "in"),
            ],
        );

        assert_eq!(nodes_remaining(&pipeline, &empty_run_state()), 3);
    }

    #[test]
    fn nodes_remaining_partial_completion() {
        let pipeline = make_pipeline(
            vec![
                make_node("a", NodeType::DocOnly, &["in"], &["out"]),
                make_node("b", NodeType::DocOnly, &["in"], &["out"]),
                make_node("c", NodeType::DocOnly, &["in"], &["out"]),
            ],
            vec![
                make_edge("a", "out", "b", "in"),
                make_edge("b", "out", "c", "in"),
            ],
        );

        let mut state = empty_run_state();
        state.nodes.insert("a".into(), completed_node("a"));
        assert_eq!(nodes_remaining(&pipeline, &state), 2);
    }

    #[test]
    fn nodes_remaining_running_counts_as_remaining() {
        let pipeline = make_pipeline(
            vec![
                make_node("a", NodeType::DocOnly, &["in"], &["out"]),
                make_node("b", NodeType::DocOnly, &["in"], &["out"]),
            ],
            vec![make_edge("a", "out", "b", "in")],
        );

        let mut state = empty_run_state();
        state.nodes.insert("a".into(), running_node("a"));
        assert_eq!(nodes_remaining(&pipeline, &state), 2);
    }

    #[test]
    fn nodes_remaining_all_completed() {
        let pipeline = make_pipeline(
            vec![
                make_node("a", NodeType::DocOnly, &["in"], &["out"]),
                make_node("b", NodeType::DocOnly, &["in"], &["out"]),
            ],
            vec![make_edge("a", "out", "b", "in")],
        );

        let mut state = empty_run_state();
        state.nodes.insert("a".into(), completed_node("a"));
        state.nodes.insert("b".into(), completed_node("b"));
        assert_eq!(nodes_remaining(&pipeline, &state), 0);
    }

    #[test]
    fn nodes_remaining_excludes_start_and_end() {
        let pipeline = make_pipeline(
            vec![
                make_node("start", NodeType::Start, &[], &["out"]),
                make_node("worker", NodeType::DocOnly, &["in"], &["out"]),
                make_node("end", NodeType::End, &["result"], &[]),
            ],
            vec![
                make_edge("start", "out", "worker", "in"),
                make_edge("worker", "out", "end", "result"),
            ],
        );

        assert_eq!(nodes_remaining(&pipeline, &empty_run_state()), 1);
    }

    #[test]
    fn nodes_remaining_with_loops_and_switches() {
        let pipeline = make_pipeline(
            vec![
                make_loop_node("loop1", 5),
                make_node("impl", NodeType::CodeMutating, &["in"], &["out"]),
                make_node("sw", NodeType::Switch, &["in"], &["pass", "default"]),
            ],
            vec![
                make_edge("loop1", "body", "impl", "in"),
                make_edge("impl", "out", "sw", "in"),
                make_edge("sw", "pass", "loop1", "break"),
            ],
        );

        let mut state = empty_run_state();
        assert_eq!(nodes_remaining(&pipeline, &state), 3);

        state.nodes.insert("impl".into(), completed_node("impl"));
        state.nodes.insert("sw".into(), completed_node("sw"));
        assert_eq!(nodes_remaining(&pipeline, &state), 1);
    }
}
