//! The Nœud à la demande and the Attente des nœuds lancés (ADR-0086): the
//! rules the scheduler and the projection share.
//!
//! An on-demand node is launched by the user's ▶ only. Until then it is
//! absent from the projection, and every edge it produces is treated as
//! **absent**: it holds no downstream barrier and kills no downstream node.

use pdo_pipeline::domain::pipeline::{EdgeDef, PipelineDef};

use crate::domain::state::{NodeStatus, RunState};

/// Whether `node_id` is an on-demand node the user has not launched (or whose
/// launch a Retry upstream invalidated): the runtime neither launches nor
/// skips it.
pub fn is_unlaunched(pipeline: &PipelineDef, run_state: &RunState, node_id: &str) -> bool {
    !run_state.nodes.contains_key(node_id)
        && pipeline
            .nodes
            .iter()
            .any(|n| n.id == node_id && n.on_demand)
}

/// Whether `edge` counts for its target's barrier and for its death: an edge
/// whose producer is an unlaunched on-demand node is treated as absent. Once
/// the producer is launched, its edges count like any other.
pub fn edge_counts(pipeline: &PipelineDef, run_state: &RunState, edge: &EdgeDef) -> bool {
    !is_unlaunched(pipeline, run_state, &edge.source.node)
}

/// Whether a node in `status` holds the end of a Run that waits for its
/// started nodes: `running`, `waiting` or `awaiting_user`.
pub fn holds_the_end(status: &NodeStatus) -> bool {
    matches!(
        status,
        NodeStatus::Running | NodeStatus::Waiting | NodeStatus::AwaitingUser
    )
}

/// How many started nodes are still alive in this Run — what a Run that
/// waits for its started nodes waits for. An on-demand node never launched is
/// absent from the projection, so it never counts.
pub fn started_nodes_alive(run_state: &RunState) -> usize {
    run_state
        .nodes
        .values()
        .filter(|n| holds_the_end(&n.status))
        .count()
}

/// How many started nodes a `Running` Run that waits for them (its frozen
/// « Attente des nœuds lancés ») still waits for past `End`, read off its
/// frozen snapshot (the projection has no pipeline to evaluate edge
/// conditions with): `End` is past once every edge into it that counts comes
/// from `Start` or from a settled node. An approximation, assumed: a settled
/// producer whose edge into `End` did not fire counts too — the scheduler,
/// which reads the conditions, is what ends the Run. Reads the « on demand »
/// option off the frozen snapshot, where the scheduler reads the Run's
/// pipeline: an « Edit for this run » of either option moves the banner only
/// from the next Run. `None` for a Run without the option (a Run re-opened by
/// a ▶ simply runs until its node is done), while the Run is not `Running`,
/// has not reached `End`, or has no started node alive.
pub fn started_nodes_waited(run_state: &RunState) -> Option<u32> {
    if !run_state.wait_for_started_nodes
        || run_state.status != crate::domain::state::RunStatus::Running
    {
        return None;
    }
    let alive = started_nodes_alive(run_state);
    if alive == 0 {
        return None;
    }
    let def = |id: &str| run_state.node_defs.iter().find(|d| d.id == id);
    let end = run_state.node_defs.iter().find(|d| d.node_type == "end")?;
    let feeders: Vec<&str> = run_state
        .edges
        .iter()
        .filter(|e| e.target_node == end.id)
        .map(|e| e.source_node.as_str())
        .filter(|src| {
            let unlaunched_on_demand =
                def(src).is_some_and(|d| d.on_demand) && !run_state.nodes.contains_key(*src);
            !unlaunched_on_demand
        })
        .collect();
    if feeders.is_empty() {
        return None;
    }
    let past_end = feeders.iter().all(|src| {
        def(src).is_some_and(|d| d.node_type == "start")
            || run_state
                .nodes
                .get(*src)
                .is_some_and(|n| n.status.is_settled_complete())
    });
    past_end.then_some(alive as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::state::NodeState;

    fn node(id: &str, status: NodeStatus) -> NodeState {
        NodeState {
            missing_skills: Vec::new(),
            skipped_skills: Vec::new(),
            skills: None,
            isolated_worktree: None,
            harness: None,
            cost: None,
            node_id: id.into(),
            status,
            iter: 1,
            started_at: None,
            completed_at: None,
            failure_reason: None,
            skip_reason: None,
            iterations: Vec::new(),
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
            missing_outputs: Vec::new(),
            delivery: None,
            awaiting: None,
        }
    }

    #[test]
    fn only_running_waiting_and_awaiting_nodes_hold_the_end() {
        let mut run = RunState::new("r".into(), "p".into());
        for (id, status) in [
            ("a", NodeStatus::Running),
            ("b", NodeStatus::Waiting),
            ("c", NodeStatus::AwaitingUser),
            ("d", NodeStatus::Completed),
            ("e", NodeStatus::Skipped),
            ("f", NodeStatus::Failed),
            ("g", NodeStatus::Stopped),
            ("h", NodeStatus::Interrupted),
        ] {
            run.nodes.insert(id.into(), node(id, status));
        }
        assert_eq!(started_nodes_alive(&run), 3);
    }
}
