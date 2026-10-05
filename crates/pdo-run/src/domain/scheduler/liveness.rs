//! Which nodes can never run any more: dead edges, dead nodes, and the nodes the
//! Run can no longer reach.

use super::*;

/// Returns `true` when `node_id` is **dead** for this run (ADR-0006 addendum): it
/// has incoming edges and every one of them is dead — its producer completed and
/// the edge did not fire, or the producer is itself dead. Death propagates
/// downstream, up to and including `End` (which is how an unrouted convergence is
/// detected instead of stalling silently).
///
/// Conservative on purpose: any still-live incoming edge means NOT dead, so the
/// convergence keeps waiting. A node present in `run_state`, or with no incoming
/// edges, is never dead.
pub(super) fn is_node_dead(
    pipeline: &PipelineDef,
    run_state: &RunState,
    node_id: &str,
    frontmatter_by_node: &HashMap<String, HashMap<String, serde_yaml::Value>>,
    vars: &HashMap<String, serde_yaml::Value>,
    visiting: &mut HashSet<String>,
) -> bool {
    if run_state.nodes.contains_key(node_id) {
        return false;
    }
    // Cycle guard: a node re-encountered mid-walk must not prop up its own
    // deadness. Treat the recursion as "not dead via this edge".
    if !visiting.insert(node_id.to_string()) {
        return false;
    }

    let incoming: Vec<&pdo_pipeline::domain::pipeline::EdgeDef> = pipeline
        .edges
        .iter()
        .filter(|e| e.target.node == node_id)
        .collect();

    if incoming.is_empty() {
        visiting.remove(node_id);
        return false;
    }

    let dead = incoming.iter().all(|edge| {
        edge_is_dead(
            pipeline,
            run_state,
            edge,
            frontmatter_by_node,
            vars,
            visiting,
        )
    });

    visiting.remove(node_id);
    dead
}

/// True when `src` is a member of a bounded loop region **still iterating** at its
/// just-completed lap: a back-edge from `src` fired this lap and the region has not
/// reached its effective `max_iter`.
///
/// [`edge_is_dead`] must consult this before pruning a member's exit edge: an exit
/// that did not fire on lap N of a live loop is not permanently dead, since lap N+1
/// can fire it. Without the guard the sweep confuses "not yet reached" with
/// "unreachable", auto-skips a node hanging off the loop's exit, and completes the
/// run with a lap in flight.
pub(super) fn bounded_loop_still_iterating(
    pipeline: &PipelineDef,
    run_state: &RunState,
    src: &str,
    source_iter: i64,
    fired: &[&pdo_pipeline::domain::pipeline::EdgeDef],
    vars: &HashMap<String, serde_yaml::Value>,
) -> bool {
    let Some(region) = crate::domain::loop_region::bounded_region_for_member(pipeline, src) else {
        return false;
    };
    // Exhausted: `handle_region_reentry` owns the exit at this lap (route or park
    // "exhausted — unrouted"); no future lap will re-fire the edge.
    let max_iter = effective_region_max_iter(run_state, region, vars);
    if source_iter >= max_iter {
        return false;
    }
    fired.iter().any(|e| {
        crate::domain::loop_region::bounded_region_reentered_by_edge(pipeline, src, &e.target.node)
            .is_some()
    })
}

/// Is a single incoming `edge` **dead** — permanently unable to deliver its
/// artifact (ADR-0011)? Separate from [`is_node_dead`] so the reachability
/// auto-skip can reason per required-input port, not only per whole node.
///
/// Live (keep waiting rather than skip) when the producer is still running, is a
/// Switch (routes by port), or is a still-iterating bounded-loop member — see
/// [`bounded_loop_still_iterating`].
pub(super) fn edge_is_dead(
    pipeline: &PipelineDef,
    run_state: &RunState,
    edge: &pdo_pipeline::domain::pipeline::EdgeDef,
    frontmatter_by_node: &HashMap<String, HashMap<String, serde_yaml::Value>>,
    vars: &HashMap<String, serde_yaml::Value>,
    visiting: &mut HashSet<String>,
) -> bool {
    let src = edge.source.node.as_str();
    let producer = pipeline.nodes.iter().find(|n| n.id == src);
    // A **skipped** producer must count as completed: otherwise control falls to
    // the "spawned but not completed" arm, keeps all its edges live, and hangs any
    // downstream join / `End` on a producer that will never run.
    let producer_completed = run_state
        .nodes
        .get(src)
        .is_some_and(|n| n.status.is_settled_complete());

    if producer_completed {
        // Switch producers route by port, not by `when:`; treat their edges as
        // live (Switch is being retired by ADR-0011 and is outside the
        // conditional-edge convergence path).
        let is_switch = producer.is_some_and(|n| n.node_type == NodeType::Switch);
        if is_switch {
            return false; // live: keep waiting
        }
        let source_iter = run_state.nodes.get(src).map(|n| n.iter).unwrap_or(1);
        let empty = HashMap::new();
        let fm = frontmatter_by_node.get(src).unwrap_or(&empty);
        let outgoing: Vec<&pdo_pipeline::domain::pipeline::EdgeDef> = pipeline
            .edges
            .iter()
            .filter(|e| e.source.node == src)
            .collect();
        let fired = edge_router::fired_edges(&outgoing, fm, vars, source_iter);
        let this_edge_fired = fired.iter().any(|f| std::ptr::eq(*f, edge));
        if this_edge_fired {
            return false; // live: the edge fired
        }
        if bounded_loop_still_iterating(pipeline, run_state, src, source_iter, &fired, vars) {
            return false;
        }
        true
    } else if run_state.nodes.contains_key(src) {
        // Spawned but not settled: outcome undecided, so the edge is still live.
        false
    } else {
        is_node_dead(
            pipeline,
            run_state,
            src,
            frontmatter_by_node,
            vars,
            visiting,
        )
    }
}

/// Nodes that are **structurally unreachable** and must be auto-skipped so the run
/// does not hang waiting on an input that can never arrive (ADR-0011). Returns
/// `(node_id, reason)`; the reason lands in the skip event (ADR-0049).
///
/// A never-started node qualifies when either (a) every incoming edge is dead, or
/// (b) it declares a `required: true` input port whose every feeding edge is dead —
/// even if the node has other live inputs.
///
/// `Start`/`End` and the structural `Loop`/`Switch` routers are never auto-skipped
/// here: `End`-unreachability is the `unrouted` convergence path. A convergence
/// node whose every branch is dead is skipped like any other node (ADR-0082).
pub fn unreachable_nodes(
    pipeline: &PipelineDef,
    run_state: &RunState,
    frontmatter_by_node: &HashMap<String, HashMap<String, serde_yaml::Value>>,
    vars: &HashMap<String, serde_yaml::Value>,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for node in &pipeline.nodes {
        if run_state.nodes.contains_key(node.id.as_str()) {
            continue;
        }
        if matches!(
            node.node_type,
            NodeType::Start | NodeType::End | NodeType::Loop | NodeType::Switch
        ) {
            continue;
        }
        let has_incoming = pipeline.edges.iter().any(|e| e.target.node == node.id);
        if !has_incoming {
            continue; // an entry point is never structurally dead
        }

        // Rule (a).
        let mut visiting = HashSet::new();
        if is_node_dead(
            pipeline,
            run_state,
            &node.id,
            frontmatter_by_node,
            vars,
            &mut visiting,
        ) {
            out.push((
                node.id.clone(),
                format!(
                    "structurally unreachable: every incoming edge to '{}' is dead \
                     (its producing branch was not taken)",
                    node.id
                ),
            ));
            continue;
        }

        // Rule (b). An emergent-input node declares no ports and is covered by (a).
        for port in node.inputs.iter().filter(|p| p.required) {
            let feeders: Vec<&pdo_pipeline::domain::pipeline::EdgeDef> = pipeline
                .edges
                .iter()
                .filter(|e| e.target.node == node.id && e.target.port == port.name)
                .collect();
            if feeders.is_empty() {
                continue;
            }
            let all_dead = feeders.iter().all(|edge| {
                let mut v = HashSet::new();
                edge_is_dead(pipeline, run_state, edge, frontmatter_by_node, vars, &mut v)
            });
            if all_dead {
                out.push((
                    node.id.clone(),
                    format!(
                        "required input '{}' of '{}' is unreachable: every edge feeding it \
                         is dead (its producing branch was not taken)",
                        port.name, node.id
                    ),
                ));
                break;
            }
        }
    }
    out
}
