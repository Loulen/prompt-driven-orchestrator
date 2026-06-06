use std::collections::{HashMap, HashSet};

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

        // A conditional edge (`when:`/`else`, ADR-0011) only fires when the
        // producer completes and the condition is evaluated — never on upstream
        // completion alone. So entry-point readiness considers ONLY unconditional
        // incoming edges. A node that *has* incoming edges but whose edges are all
        // conditional is never an entry point: it is spawned solely by the
        // producer's edge evaluation. (A node with no incoming edges at all is a
        // root and stays ready.)
        let incoming: Vec<&crate::pipeline::EdgeDef> = pipeline
            .edges
            .iter()
            .filter(|e| e.target.node == node.id)
            .collect();
        let unconditional_in: Vec<&&crate::pipeline::EdgeDef> = incoming
            .iter()
            .filter(|e| e.when.is_none() && !e.is_else)
            .collect();

        if !incoming.is_empty() && unconditional_in.is_empty() {
            continue;
        }

        let upstream: HashSet<&str> = unconditional_in
            .iter()
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

/// Detects the cycles in a pipeline's edge graph (ADR-0011 / #148).
///
/// A *cycle* is either a strongly-connected component of two or more nodes, or a
/// single node carrying a self-edge (a node looping on itself is a valid bounded
/// region of one member). Each returned cycle is the set of its member node ids,
/// ordered by their position in `pipeline.nodes` for determinism. The list of
/// cycles is itself ordered by the position of each cycle's first member.
///
/// This is the topological signature a bounded loop region is auto-materialized
/// from, so that no cycle is ever accidentally unbounded.
pub fn detect_cycles(pipeline: &PipelineDef) -> Vec<Vec<String>> {
    // Index nodes for stable ordering and adjacency by id.
    let order: HashMap<&str, usize> = pipeline
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();

    // Adjacency list (source -> targets), restricted to known nodes.
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut has_self_edge: HashSet<&str> = HashSet::new();
    for edge in &pipeline.edges {
        let (s, t) = (edge.source.node.as_str(), edge.target.node.as_str());
        if !order.contains_key(s) || !order.contains_key(t) {
            continue;
        }
        if s == t {
            has_self_edge.insert(s);
        }
        adj.entry(s).or_default().push(t);
    }

    // Tarjan's strongly-connected-components, iterative-friendly via recursion on
    // a bounded graph (pipelines are small).
    struct Tarjan<'a> {
        adj: &'a HashMap<&'a str, Vec<&'a str>>,
        index: HashMap<&'a str, usize>,
        lowlink: HashMap<&'a str, usize>,
        on_stack: HashSet<&'a str>,
        stack: Vec<&'a str>,
        next_index: usize,
        sccs: Vec<Vec<&'a str>>,
    }

    impl<'a> Tarjan<'a> {
        fn strongconnect(&mut self, v: &'a str) {
            self.index.insert(v, self.next_index);
            self.lowlink.insert(v, self.next_index);
            self.next_index += 1;
            self.stack.push(v);
            self.on_stack.insert(v);

            if let Some(targets) = self.adj.get(v) {
                for &w in targets {
                    if !self.index.contains_key(w) {
                        self.strongconnect(w);
                        let low_w = self.lowlink[w];
                        let low_v = self.lowlink[v];
                        self.lowlink.insert(v, low_v.min(low_w));
                    } else if self.on_stack.contains(w) {
                        let idx_w = self.index[w];
                        let low_v = self.lowlink[v];
                        self.lowlink.insert(v, low_v.min(idx_w));
                    }
                }
            }

            if self.lowlink[v] == self.index[v] {
                let mut component = Vec::new();
                while let Some(w) = self.stack.pop() {
                    self.on_stack.remove(w);
                    component.push(w);
                    if w == v {
                        break;
                    }
                }
                self.sccs.push(component);
            }
        }
    }

    let mut tarjan = Tarjan {
        adj: &adj,
        index: HashMap::new(),
        lowlink: HashMap::new(),
        on_stack: HashSet::new(),
        stack: Vec::new(),
        next_index: 0,
        sccs: Vec::new(),
    };
    for node in &pipeline.nodes {
        let id = node.id.as_str();
        if !tarjan.index.contains_key(id) {
            tarjan.strongconnect(id);
        }
    }

    // Keep components that are cyclic: size >= 2, or a single self-edged node.
    let mut cycles: Vec<Vec<String>> = tarjan
        .sccs
        .into_iter()
        .filter(|comp| comp.len() >= 2 || comp.first().is_some_and(|n| has_self_edge.contains(n)))
        .map(|mut comp| {
            comp.sort_by_key(|id| order.get(id).copied().unwrap_or(usize::MAX));
            comp.into_iter().map(String::from).collect()
        })
        .collect();

    // Order cycles by their first member's node position.
    cycles.sort_by_key(|members| {
        members
            .first()
            .and_then(|id| order.get(id.as_str()).copied())
            .unwrap_or(usize::MAX)
    });

    cycles
}

/// Identifies the *entry* node of a loop region (ADR-0011 / #148): the member
/// that has an incoming edge from a node outside the region. The entry is the
/// node re-spawned once per lap.
///
/// Returns the first such member in `members` order, or `None` if no member is
/// fed from outside (a closed island — degenerate, no defined entry).
pub fn region_entry(pipeline: &PipelineDef, members: &[String]) -> Option<String> {
    let member_set: HashSet<&str> = members.iter().map(String::as_str).collect();
    members
        .iter()
        .find(|m| {
            pipeline
                .edges
                .iter()
                .any(|e| e.target.node == **m && !member_set.contains(e.source.node.as_str()))
        })
        .cloned()
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
    use crate::pipeline::{EdgeDef, EdgeEndpoint, NodeDef, NodeType, Port, PortType};
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

    fn make_pipeline(nodes: Vec<NodeDef>, edges: Vec<EdgeDef>) -> PipelineDef {
        PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes,
            edges,
            loops: Vec::new(),
            prompt_required: true,
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
    fn ready_nodes_skips_target_reached_only_by_conditional_edge() {
        // ADR-0011: a node reached only via a conditional (`when:`) edge must not
        // be spawned as an entry point. Whether it runs depends on the edge
        // condition, evaluated when the producer completes — not on upstream
        // completion alone. Otherwise the guarded branch would always fire.
        let pipeline = make_pipeline(
            vec![
                make_node("classifier", NodeType::DocOnly, &["task"], &["triage"]),
                make_node("hotfix", NodeType::CodeMutating, &["triage"], &["patch"]),
            ],
            vec![make_cond_edge(
                "classifier",
                "triage",
                "hotfix",
                "triage",
                Some("severity: { eq: high }"),
                false,
            )],
        );

        let mut state = empty_run_state();
        state
            .nodes
            .insert("classifier".into(), completed_node("classifier"));

        let ready = ready_nodes(&pipeline, &state);
        assert!(
            !ready.contains(&"hotfix".to_string()),
            "conditional-edge target must not be an entry point: {ready:?}"
        );
    }

    #[test]
    fn ready_nodes_skips_target_reached_only_by_else_edge() {
        let pipeline = make_pipeline(
            vec![
                make_node("classifier", NodeType::DocOnly, &["task"], &["triage"]),
                make_node("backlog", NodeType::DocOnly, &["triage"], &["note"]),
            ],
            vec![make_cond_edge(
                "classifier",
                "triage",
                "backlog",
                "triage",
                None,
                true,
            )],
        );

        let mut state = empty_run_state();
        state
            .nodes
            .insert("classifier".into(), completed_node("classifier"));

        let ready = ready_nodes(&pipeline, &state);
        assert!(
            !ready.contains(&"backlog".to_string()),
            "else-edge target must not be an entry point: {ready:?}"
        );
    }

    #[test]
    fn ready_nodes_spawns_target_with_an_unconditional_incoming_edge() {
        // A node with at least one plain (unconditional) incoming edge is still
        // a normal entry point once that upstream completes.
        let pipeline = make_pipeline(
            vec![
                make_node("a", NodeType::DocOnly, &["task"], &["out"]),
                make_node("b", NodeType::DocOnly, &["in"], &["out"]),
            ],
            vec![make_edge("a", "out", "b", "in")],
        );

        let mut state = empty_run_state();
        state.nodes.insert("a".into(), completed_node("a"));

        let ready = ready_nodes(&pipeline, &state);
        assert!(ready.contains(&"b".to_string()));
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

    #[test]
    fn detect_cycles_finds_a_two_node_cycle() {
        // ADR-0011 / #148: a bounded loop is born by auto-detection of a cycle.
        // implementer -> reviewer -> implementer is one cycle of two members.
        let pipeline = make_pipeline(
            vec![
                make_node("impl", NodeType::CodeMutating, &["review"], &["code"]),
                make_node("rev", NodeType::DocOnly, &["code"], &["review"]),
            ],
            vec![
                make_edge("impl", "code", "rev", "code"),
                make_edge("rev", "review", "impl", "review"),
            ],
        );

        let cycles = detect_cycles(&pipeline);
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0], vec!["impl".to_string(), "rev".to_string()]);
    }

    #[test]
    fn detect_cycles_finds_a_self_edge() {
        // A self-looping member is a valid bounded region of ONE member
        // (ADR-0011 / #148: self-edge included).
        let pipeline = make_pipeline(
            vec![
                make_node("a", NodeType::DocOnly, &["in"], &["out"]),
                make_node(
                    "worker",
                    NodeType::CodeMutating,
                    &["seed", "again"],
                    &["out"],
                ),
                make_node("b", NodeType::DocOnly, &["in"], &["out"]),
            ],
            vec![
                make_edge("a", "out", "worker", "seed"),
                make_edge("worker", "out", "worker", "again"),
                make_edge("worker", "out", "b", "in"),
            ],
        );

        let cycles = detect_cycles(&pipeline);
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0], vec!["worker".to_string()]);
    }

    #[test]
    fn detect_cycles_ignores_acyclic_graph() {
        // A pure DAG has no cycles — nothing to auto-materialize.
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
        assert!(detect_cycles(&pipeline).is_empty());
    }

    #[test]
    fn region_entry_is_the_member_fed_from_outside() {
        // The region entry is the member with an incoming edge from a non-member
        // (the node where the loop is entered, re-spawned once per lap).
        let members = vec!["impl".to_string(), "rev".to_string()];
        let pipeline = make_pipeline(
            vec![
                make_node("start", NodeType::Start, &[], &["user_prompt"]),
                make_node(
                    "impl",
                    NodeType::CodeMutating,
                    &["task", "review"],
                    &["code"],
                ),
                make_node("rev", NodeType::DocOnly, &["code"], &["review"]),
            ],
            vec![
                make_edge("start", "user_prompt", "impl", "task"),
                make_edge("impl", "code", "rev", "code"),
                make_edge("rev", "review", "impl", "review"),
            ],
        );
        assert_eq!(region_entry(&pipeline, &members), Some("impl".to_string()));
    }
}
