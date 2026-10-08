//! How a Run ends when nodes it launched are still alive (ADR-0086): the
//! « Attente des nœuds lancés » defers the completion at `End`, and the end
//! is evaluated again when a later completion leaves no started node alive.

use super::*;
use crate::domain::on_demand::started_nodes_alive;
use crate::domain::state::RunStatus;

/// Drop the `Complete` of `actions` while a Run that waits for its started
/// nodes must keep waiting: a node is still `running`, `waiting` or
/// `awaiting_user`, or this very pass spawns one. A pipeline that does not
/// wait gets its actions back untouched — the Run ends at `End`, as before.
pub fn defer_completion_for_started_nodes(
    pipeline: &PipelineDef,
    run_state: &RunState,
    mut actions: Vec<SchedulerAction>,
) -> Vec<SchedulerAction> {
    if !pipeline.wait_for_started_nodes || !actions.contains(&SchedulerAction::Complete) {
        return actions;
    }
    let spawns = actions
        .iter()
        .any(|a| matches!(a, SchedulerAction::Spawn { .. }));
    if spawns || started_nodes_alive(run_state) > 0 {
        actions.retain(|a| *a != SchedulerAction::Complete);
    }
    actions
}

/// The end of the Run, evaluated again after a completion that did not reach
/// `End` itself: the completion of the last started node of a Run that waits
/// for them, or of a node launched after the Run went past `End` (an on-demand
/// node, a Retry), which re-opened it.
///
/// `Some` once the Run is `Running`, no started node is alive, every edge into
/// `End` that counts is resolved and one of them fired: `Halt` when a fired
/// edge into `End` carries a `reason:`, `Complete` otherwise. `None` while the
/// Run has not reached `End`, so a Run in mid-course is never ended here.
pub fn end_reached_action(
    pipeline: &PipelineDef,
    run_state: &RunState,
    frontmatter_by_node: &HashMap<String, HashMap<String, serde_yaml::Value>>,
    vars: &HashMap<String, serde_yaml::Value>,
) -> Option<SchedulerAction> {
    if run_state.status != RunStatus::Running || started_nodes_alive(run_state) > 0 {
        return None;
    }
    let end_id = pipeline
        .nodes
        .iter()
        .find(|n| n.node_type == NodeType::End)?
        .id
        .as_str();
    if !check_all_upstream_completed(pipeline, run_state, end_id, "", frontmatter_by_node, vars) {
        return None;
    }

    let empty = HashMap::new();
    let mut fired_into_end = Vec::new();
    let producers: HashSet<&str> = pipeline
        .edges
        .iter()
        .filter(|e| e.target.node == end_id)
        .filter(|e| crate::domain::on_demand::edge_counts(pipeline, run_state, e))
        .map(|e| e.source.node.as_str())
        .collect();
    for src in producers {
        let Some(producer) = run_state.nodes.get(src) else {
            continue;
        };
        if !producer.status.is_settled_complete() {
            continue;
        }
        let outgoing: Vec<&pdo_pipeline::domain::pipeline::EdgeDef> = pipeline
            .edges
            .iter()
            .filter(|e| e.source.node == src)
            .collect();
        let fields = frontmatter_by_node.get(src).unwrap_or(&empty);
        for edge in edge_router::fired_edges(&outgoing, fields, vars, producer.iter) {
            if edge.target.node == end_id {
                fired_into_end.push((src, producer.iter, edge.reason.clone(), fields));
            }
        }
    }
    fired_into_end.sort_by(|a, b| a.0.cmp(b.0));

    if fired_into_end.is_empty() {
        return None;
    }
    if let Some((src, iter, Some(reason), fields)) = fired_into_end
        .iter()
        .find(|(_, _, reason, _)| reason.is_some())
    {
        let message = condition::render_halt_message(
            reason,
            &condition::HaltContext {
                iter: *iter,
                node_id: (*src).to_string(),
                variables: vars.clone(),
                fields: (*fields).clone(),
            },
        );
        return Some(SchedulerAction::Halt { message });
    }
    Some(SchedulerAction::Complete)
}
