//! How a Run ends when nodes it launched are still alive (ADR-0086): the
//! « Attente des nœuds lancés » defers the completion at `End`, and the end
//! is evaluated again when a later completion leaves no started node alive.

use super::*;
use crate::domain::on_demand::started_nodes_alive;
use crate::domain::state::RunStatus;

/// Drop the action that ends the Run (`Complete`, or the `Halt` of an edge
/// into `End` with a `reason:`) while a Run that waits for its started nodes
/// must keep waiting: a node is still `running`, `waiting` or
/// `awaiting_user`, or this very pass spawns one. A pipeline that does not
/// wait gets its actions back untouched — the Run ends at `End`, as before.
pub fn defer_end_for_started_nodes(
    pipeline: &PipelineDef,
    run_state: &RunState,
    mut actions: Vec<SchedulerAction>,
) -> Vec<SchedulerAction> {
    if !pipeline.wait_for_started_nodes || !actions.iter().any(ends_the_run) {
        return actions;
    }
    let spawns = actions
        .iter()
        .any(|a| matches!(a, SchedulerAction::Spawn { .. }));
    if spawns || started_nodes_alive(run_state) > 0 {
        actions.retain(|a| !ends_the_run(a));
    }
    actions
}

fn ends_the_run(action: &SchedulerAction) -> bool {
    matches!(
        action,
        SchedulerAction::Complete | SchedulerAction::Halt { .. }
    )
}

/// The end of the Run, evaluated again after a completion that did not reach
/// `End` itself: the completion of the last started node of a Run that waits
/// for them, or of a node launched after the Run went past `End` (an on-demand
/// node, a Retry), which re-opened it.
///
/// `Some` once the Run is `Running`, no started node is alive and the routing
/// of a settled producer of `End` — replayed as it ran, forced routes, Switch
/// ports and regions included — ends the Run: `Halt` when one of them halts,
/// `Complete` otherwise. `None` while the Run has not reached `End`, so a Run
/// in mid-course is never ended here.
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
        .clone();
    // The producers that may route to `End`: the sources of its edges, and the
    // sources a `force_route` sends there. A Switch routes by the branch it
    // chose, through the producer it was evaluated for.
    let mut producers: Vec<String> = pipeline
        .edges
        .iter()
        .filter(|e| e.target.node == end_id)
        .map(|e| e.source.node.clone())
        .chain(
            run_state
                .forced_routes
                .iter()
                .filter(|(_, target)| **target == end_id)
                .map(|(source, _)| source.clone()),
        )
        .collect();
    producers.sort();
    producers.dedup();

    let empty = HashMap::new();
    let mut ends = Vec::new();
    for producer in producers {
        if let Some(switch) = run_state.switch_states.get(&producer) {
            // The Switch already chose: its edge into `End` on the chosen port
            // is the one routing took.
            let routed = pipeline.edges.iter().find(|e| {
                e.source.node == switch.switch_node_id
                    && e.target.node == end_id
                    && e.source.carries(&switch.chosen_branch)
            });
            if let Some(edge) = routed {
                if check_all_upstream_completed(
                    pipeline,
                    run_state,
                    &end_id,
                    &producer,
                    frontmatter_by_node,
                    vars,
                ) {
                    ends.push(match &edge.reason {
                        Some(reason) => SchedulerAction::Halt {
                            message: condition::render_halt_message(
                                reason,
                                &condition::HaltContext {
                                    iter: 1,
                                    node_id: producer.clone(),
                                    variables: vars.clone(),
                                    fields: HashMap::new(),
                                },
                            ),
                        },
                        None => SchedulerAction::Complete,
                    });
                }
            }
            continue;
        }
        let settled = run_state
            .nodes
            .get(&producer)
            .is_some_and(|n| n.status.is_settled_complete());
        if !settled {
            continue;
        }
        let fields = frontmatter_by_node.get(&producer).unwrap_or(&empty);
        ends.extend(
            route_outgoing_edges(
                pipeline,
                run_state,
                &producer,
                vars,
                fields,
                frontmatter_by_node,
                None,
            )
            .into_iter()
            .filter(ends_the_run),
        );
    }
    ends.iter()
        .find(|a| matches!(a, SchedulerAction::Halt { .. }))
        .or_else(|| ends.first())
        .cloned()
}
