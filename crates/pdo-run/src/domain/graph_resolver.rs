use std::collections::HashSet;

use crate::domain::state::RunState;
use pdo_pipeline::domain::pipeline::{NodeType, PipelineDef};

/// Returns the IDs of nodes that are ready to be spawned: all upstream
/// dependencies completed, node not yet started, and not a control-flow
/// construct (Start, End, Loop, Switch).
pub fn ready_nodes(pipeline: &PipelineDef, run_state: &RunState) -> Vec<String> {
    let mut ready = Vec::new();

    for node in &pipeline.nodes {
        if matches!(
            node.node_type,
            NodeType::Start | NodeType::End | NodeType::Loop | NodeType::Switch
        ) {
            continue;
        }
        if run_state.nodes.contains_key(&node.id) {
            continue;
        }
        // ADR-0086: an on-demand node is launched by the user's ▶ only — never
        // as a root, never through an incoming edge.
        if node.on_demand {
            continue;
        }

        // A `collection` region member (ADR-0011 / #269) is spawned by the
        // region fan-out (one lap per item), never by the readiness sweep —
        // except in the degenerate case where its only external feed is Start
        // (no producer completion will ever fire the fan-out edge, so the
        // sweep spawning it plainly at iter 1 beats a silent stall).
        if let Some(region) =
            crate::domain::loop_region::collection_region_for_member(pipeline, &node.id)
        {
            let fed_by_external_producer = pipeline.edges.iter().any(|e| {
                e.target.node == node.id
                    && !region.members.contains(&e.source.node)
                    && !pipeline
                        .nodes
                        .iter()
                        .any(|n| n.id == e.source.node && n.node_type == NodeType::Start)
            });
            if fed_by_external_producer {
                continue;
            }
        }

        // A conditional edge (`when:`/`else`, ADR-0011) only fires when the
        // producer completes and the condition is evaluated — never on upstream
        // completion alone. So entry-point readiness considers ONLY unconditional
        // incoming edges. A node that *has* incoming edges but whose edges are all
        // conditional is never an entry point: it is spawned solely by the
        // producer's edge evaluation. (A node with no incoming edges at all is a
        // root and stays ready.)
        //
        // ADR-0086: an edge from an on-demand node not launched yet is absent
        // for the barrier. A node fed by such edges alone is no root: it waits
        // for them, like a node fed by a conditional edge.
        let wired = pipeline.edges.iter().any(|e| e.target.node == node.id);
        let incoming: Vec<&pdo_pipeline::domain::pipeline::EdgeDef> = pipeline
            .edges
            .iter()
            .filter(|e| e.target.node == node.id)
            .filter(|e| crate::domain::on_demand::edge_counts(pipeline, run_state, e))
            .collect();
        if wired && incoming.is_empty() {
            continue;
        }
        let unconditional_in: Vec<&&pdo_pipeline::domain::pipeline::EdgeDef> = incoming
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

        // ADR-0082: a node is ready once at least one of its edges fired. A
        // pruned producer fires nothing, so a node whose every unconditional
        // feeder is pruned is not ready by readiness: a live conditional edge
        // into it spawns it, or it dies and is pruned in turn.
        if !unconditional_in.is_empty()
            && unconditional_in
                .iter()
                .all(|e| run_state.is_pruned(&e.source.node))
        {
            continue;
        }

        // ADR-0082: a node is ready once EVERY incoming edge is resolved. A
        // conditional edge whose producer has not decided yet — not run, still
        // at work, or a member of a bounded loop that can still start a lap (its
        // guarded exit may fire on a later lap) — holds the node: the producer's
        // routing spawns it once it decides (the frontmatter-aware half of this
        // read is `dispatcher::hold_pending_loop_exits`). A pruned producer has decided. A
        // feedback edge (its producer downstream of the node: a loop's back-edge,
        // a self-edge) is no precondition of the node's first run.
        let conditional_undecided = incoming
            .iter()
            .filter(|e| e.when.is_some() || e.is_else)
            .map(|e| e.source.node.as_str())
            .filter(|src| *src != node.id && !reaches(pipeline, &node.id, src))
            .any(|src| conditional_source_undecided(pipeline, run_state, src));
        if conditional_undecided {
            continue;
        }

        let all_completed = upstream.iter().all(|src| {
            // A collection-region member upstream is a BARRIER input (#269):
            // it counts as completed only once the whole region is done, never
            // on a per-lap completion mid-fan-out.
            if let Some(region) =
                crate::domain::loop_region::collection_region_for_member(pipeline, src)
            {
                return run_state
                    .collection_states
                    .get(region.id.as_str())
                    .is_some_and(|cs| cs.done);
            }
            run_state
                .nodes
                .get(*src)
                .is_some_and(|n| n.status.is_settled_complete())
        });
        if all_completed {
            ready.push(node.id.clone());
        }
    }

    ready
}

/// Has the producer of a conditional edge not decided whether it fires yet?
/// Read off the projection alone: it has not settled, or it is a member of a
/// bounded region still in flight
/// ([`crate::domain::loop_region::bounded_region_in_flight`]). A FAIL lap whose
/// next lap is not in the log yet looks idle here; the sweep completes this
/// read with the producers' frontmatter
/// ([`crate::domain::scheduler::dispatcher::hold_pending_loop_exits`]).
fn conditional_source_undecided(pipeline: &PipelineDef, run_state: &RunState, src: &str) -> bool {
    if run_state.is_pruned(src) {
        return false;
    }
    let settled = run_state
        .nodes
        .get(src)
        .is_some_and(|n| n.status.is_settled_complete());
    if !settled {
        return true;
    }
    crate::domain::loop_region::bounded_region_for_member(pipeline, src).is_some_and(|region| {
        crate::domain::loop_region::bounded_region_in_flight(pipeline, region, run_state)
    })
}

/// True when a directed path of forward edges leads from `from` to `to`
/// (self-edges excluded: a node does not reach itself through its own edge).
pub(crate) fn reaches(pipeline: &PipelineDef, from: &str, to: &str) -> bool {
    let mut visited: HashSet<&str> = HashSet::new();
    let mut queue: Vec<&str> = vec![from];
    while let Some(current) = queue.pop() {
        for edge in pipeline
            .edges
            .iter()
            .filter(|e| e.source.node == current && e.target.node != current)
        {
            let next = edge.target.node.as_str();
            if next == to {
                return true;
            }
            if visited.insert(next) {
                queue.push(next);
            }
        }
    }
    false
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

/// Computes the set of nodes that form the body subgraph of a Loop node. BFS
/// from the loop's "body" output port, collecting all reachable nodes until
/// hitting the loop's own break/done ports. Nested loops are treated as opaque
/// (included but their internals are not traversed).
pub fn compute_body_subgraph(
    pipeline: &PipelineDef,
    loop_node_id: &str,
) -> Result<HashSet<String>, BodyResolutionError> {
    pipeline
        .nodes
        .iter()
        .find(|n| n.id == loop_node_id && n.node_type == NodeType::Loop)
        .ok_or_else(|| BodyResolutionError::LoopNotFound(loop_node_id.to_string()))?;

    let body_targets: Vec<&str> = pipeline
        .edges
        .iter()
        .filter(|e| e.source.node == loop_node_id && e.source.carries("body"))
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
            .any(|n| n.id == current && n.node_type == NodeType::Loop);
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
///
/// The no-exclusions shorthand for [`downstream_subgraph_excluding`]. Production
/// callers (the retry path) always pass the bounded-region back-edges to exclude,
/// so this bare form is now exercised only by this module's unit tests — kept as
/// the documented base case.
#[allow(dead_code)]
pub fn downstream_subgraph(pipeline: &PipelineDef, node_id: &str) -> HashSet<String> {
    downstream_subgraph_excluding(pipeline, node_id, &HashSet::new())
}

/// Like [`downstream_subgraph`], but skips the edges whose global index (into
/// `pipeline.edges`) is in `excluded_edges`. The starting node is NOT included.
///
/// The retry path uses this to walk a loop's *forward* slice only: excluding a
/// bounded region's re-entry (back) edges (see
/// [`crate::domain::loop_region::bounded_reentry_edges`]) stops the walk from stepping
/// backward through the entry into an earlier member of the *same lap* — work
/// that a retry must leave untouched. In an acyclic graph "reachable-from" is
/// "downstream"; inside a cycle it is not, and this is the correction.
pub fn downstream_subgraph_excluding(
    pipeline: &PipelineDef,
    node_id: &str,
    excluded_edges: &HashSet<usize>,
) -> HashSet<String> {
    let mut visited = HashSet::new();
    visited.insert(node_id.to_string());
    let mut queue = vec![node_id.to_string()];

    while let Some(current) = queue.pop() {
        for (idx, edge) in pipeline.edges.iter().enumerate() {
            if excluded_edges.contains(&idx) {
                continue;
            }
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
// #494: exercised only by this module's unit tests since demotion; kept as a tested helper.
#[allow(dead_code)]
pub fn nodes_remaining(pipeline: &PipelineDef, run_state: &RunState) -> usize {
    pipeline
        .nodes
        .iter()
        .filter(|n| !matches!(n.node_type, NodeType::Start | NodeType::End))
        .filter(|n| {
            !run_state
                .nodes
                .get(&n.id)
                .is_some_and(|ns| ns.status.is_settled_complete())
        })
        .count()
}

#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::state::{NodeState, NodeStatus};
    use pdo_pipeline::domain::pipeline::{
        EdgeDef, EdgeEndpoint, EdgeSource, NodeDef, NodeType, Port, PortType,
    };
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    fn make_node(id: &str, node_type: NodeType, inputs: &[&str], outputs: &[&str]) -> NodeDef {
        NodeDef {
            free_live_artifacts: false,
            live_artifacts: Vec::new(),
            skills: Vec::new(),
            isolated_worktree: None,
            id: id.into(),
            name: id.into(),
            node_type,
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

    fn make_loop_node(id: &str, max_iter: i64) -> NodeDef {
        NodeDef {
            free_live_artifacts: false,
            live_artifacts: Vec::new(),
            skills: Vec::new(),
            isolated_worktree: None,
            id: id.into(),
            name: id.into(),
            node_type: NodeType::Loop,
            inputs: vec![
                Port {
                    name: "in".into(),
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                },
                Port {
                    name: "break".into(),
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                },
            ],
            outputs: vec![
                Port {
                    name: "body".into(),
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                },
                Port {
                    name: "done".into(),
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                },
            ],
            interactive: false,
            view: None,
            max_iter: Some(serde_yaml::Value::Number(serde_yaml::Number::from(
                max_iter,
            ))),
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

    fn make_cond_edge(
        src_node: &str,
        src_port: &str,
        tgt_node: &str,
        tgt_port: &str,
        when: Option<&str>,
        is_else: bool,
    ) -> EdgeDef {
        EdgeDef {
            source: EdgeSource::single(src_node, src_port),
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
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
        }
    }

    fn empty_run_state() -> RunState {
        RunState::new("run-1".into(), "test".into())
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

    #[test]
    fn ready_nodes_skips_switch() {
        let pipeline = make_pipeline(
            vec![
                make_node("upstream", NodeType::Agent, &["in"], &["out"]),
                make_node("sw", NodeType::Switch, &["in"], &["pass", "default"]),
                make_node("downstream", NodeType::Agent, &["in"], &["out"]),
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
                make_node("classifier", NodeType::Agent, &["task"], &["triage"]),
                make_node("hotfix", NodeType::Agent, &["triage"], &["patch"]),
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
                make_node("classifier", NodeType::Agent, &["task"], &["triage"]),
                make_node("backlog", NodeType::Agent, &["triage"], &["note"]),
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
        let pipeline = make_pipeline(
            vec![
                make_node("a", NodeType::Agent, &["task"], &["out"]),
                make_node("b", NodeType::Agent, &["in"], &["out"]),
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
                make_node("planner", NodeType::Agent, &["task"], &["plan"]),
                make_node("implementer", NodeType::Agent, &["plan"], &["summary"]),
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
                make_node("a", NodeType::Agent, &["task"], &["out"]),
                make_node("b", NodeType::Agent, &["task"], &["out"]),
                make_node("c", NodeType::Agent, &["in-a", "in-b"], &["out"]),
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
                make_node("worker", NodeType::Agent, &["in"], &["out"]),
            ],
            vec![make_edge("loop1", "body", "worker", "in")],
        );

        let ready = ready_nodes(&pipeline, &empty_run_state());
        assert!(!ready.contains(&"loop1".to_string()));
    }

    #[test]
    fn body_subgraph_linear_body() {
        let pipeline = make_pipeline(
            vec![
                make_loop_node("loop1", 5),
                make_node("a", NodeType::Agent, &["in"], &["out"]),
                make_node("b", NodeType::Agent, &["in"], &["out"]),
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
                make_node("inner_worker", NodeType::Agent, &["in"], &["out"]),
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
            vec![make_node("a", NodeType::Agent, &["in"], &["out"])],
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
                make_node("a", NodeType::Agent, &["in"], &["out"]),
                make_node("b", NodeType::Agent, &["in"], &["out"]),
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
                make_node("impl", NodeType::Agent, &["in"], &["out"]),
                make_node("reviewer", NodeType::Agent, &["in"], &["review"]),
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

    #[test]
    fn downstream_linear_chain() {
        let pipeline = make_pipeline(
            vec![
                make_node("a", NodeType::Agent, &["in"], &["out"]),
                make_node("b", NodeType::Agent, &["in"], &["out"]),
                make_node("c", NodeType::Agent, &["in"], &["out"]),
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
                make_node("a", NodeType::Agent, &["in"], &["out"]),
                make_node("b", NodeType::Agent, &["in"], &["out"]),
                make_node("c", NodeType::Agent, &["in"], &["out"]),
                make_node("d", NodeType::Agent, &["in-b", "in-c"], &["out"]),
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
                make_node("a", NodeType::Agent, &["in"], &["out"]),
                make_node("b", NodeType::Agent, &["in"], &["out"]),
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
                make_node("impl", NodeType::Agent, &["in"], &["out"]),
                make_node("reviewer", NodeType::Agent, &["in"], &["review"]),
                make_node("downstream", NodeType::Agent, &["in"], &["out"]),
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
    fn downstream_excluding_stops_at_the_back_edge() {
        // The retry fix (loop-aware invalidation): a review loop `impl -> rev`
        // with a back-edge `rev -> impl` and an exit `rev -> end`. The plain walk
        // from `rev` follows the back-edge into `impl` (a same-lap upstream member
        // that a retry must NOT reset). Excluding the back-edge's index leaves the
        // walk with the forward slice only: `end`, never `impl`.
        let pipeline = make_pipeline(
            vec![
                make_node("impl", NodeType::Agent, &["task", "review"], &["code"]),
                make_node("rev", NodeType::Agent, &["code"], &["review"]),
                make_node("end", NodeType::Agent, &["result"], &[]),
            ],
            vec![
                make_edge("impl", "code", "rev", "code"),     // 0
                make_edge("rev", "review", "impl", "review"), // 1 back-edge
                make_edge("rev", "review", "end", "result"),  // 2 exit
            ],
        );

        // Plain walk: back-edge drags `impl` in.
        let plain = downstream_subgraph(&pipeline, "rev");
        assert!(plain.contains("impl"), "plain walk follows the back-edge");

        // Loop-aware walk: exclude the back-edge (index 1).
        let mut excluded = HashSet::new();
        excluded.insert(1usize);
        let ds = downstream_subgraph_excluding(&pipeline, "rev", &excluded);
        let expected: HashSet<String> = ["end"].iter().map(|s| s.to_string()).collect();
        assert_eq!(ds, expected, "same-lap upstream `impl` must be spared");
    }

    #[test]
    fn downstream_excluding_from_entry_still_covers_the_whole_body() {
        // Excluding the back-edge must NOT starve a retry of the entry: from
        // `impl` (the region entry) the forward path reaches `rev` and `end`
        // regardless — the excluded back-edge only ever pointed back at `impl`.
        let pipeline = make_pipeline(
            vec![
                make_node("impl", NodeType::Agent, &["task", "review"], &["code"]),
                make_node("rev", NodeType::Agent, &["code"], &["review"]),
                make_node("end", NodeType::Agent, &["result"], &[]),
            ],
            vec![
                make_edge("impl", "code", "rev", "code"),     // 0
                make_edge("rev", "review", "impl", "review"), // 1 back-edge
                make_edge("rev", "review", "end", "result"),  // 2 exit
            ],
        );

        let mut excluded = HashSet::new();
        excluded.insert(1usize);
        let ds = downstream_subgraph_excluding(&pipeline, "impl", &excluded);
        let expected: HashSet<String> = ["rev", "end"].iter().map(|s| s.to_string()).collect();
        assert_eq!(ds, expected);
    }

    #[test]
    fn downstream_nested_loops() {
        let pipeline = make_pipeline(
            vec![
                make_loop_node("outer", 3),
                make_loop_node("inner", 5),
                make_node("worker", NodeType::Agent, &["in"], &["out"]),
                make_node("final", NodeType::Agent, &["in"], &["out"]),
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
                make_node("pass-handler", NodeType::Agent, &["in"], &["out"]),
                make_node("fail-handler", NodeType::Agent, &["in"], &["out"]),
                make_node("merge", NodeType::Agent, &["in-p", "in-f"], &["out"]),
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
                make_node("a", NodeType::Agent, &["in"], &["out"]),
                make_node("b", NodeType::Agent, &["in"], &["out"]),
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

    #[test]
    fn nodes_remaining_all_pending() {
        let pipeline = make_pipeline(
            vec![
                make_node("a", NodeType::Agent, &["in"], &["out"]),
                make_node("b", NodeType::Agent, &["in"], &["out"]),
                make_node("c", NodeType::Agent, &["in"], &["out"]),
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
                make_node("a", NodeType::Agent, &["in"], &["out"]),
                make_node("b", NodeType::Agent, &["in"], &["out"]),
                make_node("c", NodeType::Agent, &["in"], &["out"]),
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
                make_node("a", NodeType::Agent, &["in"], &["out"]),
                make_node("b", NodeType::Agent, &["in"], &["out"]),
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
                make_node("a", NodeType::Agent, &["in"], &["out"]),
                make_node("b", NodeType::Agent, &["in"], &["out"]),
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
                make_node("worker", NodeType::Agent, &["in"], &["out"]),
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
                make_node("impl", NodeType::Agent, &["in"], &["out"]),
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
    fn region_entry_is_the_member_fed_from_outside() {
        // The region entry is the member with an incoming edge from a non-member
        // (the node where the loop is entered, re-spawned once per lap).
        let members = vec!["impl".to_string(), "rev".to_string()];
        let pipeline = make_pipeline(
            vec![
                make_node("start", NodeType::Start, &[], &["user_prompt"]),
                make_node("impl", NodeType::Agent, &["task", "review"], &["code"]),
                make_node("rev", NodeType::Agent, &["code"], &["review"]),
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
