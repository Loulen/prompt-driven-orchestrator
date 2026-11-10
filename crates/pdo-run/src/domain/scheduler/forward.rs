//! The iteration of a node a fired forward edge spawns, if any.

use super::*;

/// Decides the iter for a generic forward spawn of `target_id` after `source_id`
/// completed — or `None` when the target must not spawn:
///
/// - never run → iter 1;
/// - fed by an on-demand node and already launched → never re-run (ADR-0086);
/// - already ran → re-run ONLY when the fired edge closes an emergent cycle (the
///   target reaches the source through forward edges), at `iter + 1`. Re-spawning
///   a node reached only by forward edges is the "feeder dragged into a lap" bug;
/// - a bounded-region member is never spawned past its effective `max_iter`;
/// - a pure self-edge is inert outside a region.
pub(super) fn forward_spawn_iter(
    pipeline: &PipelineDef,
    run_state: &RunState,
    source_id: &str,
    target_id: &str,
    resolved_vars: &HashMap<String, serde_yaml::Value>,
) -> Option<i64> {
    if source_id == target_id {
        return None;
    }

    let proposed = match run_state.nodes.get(target_id) {
        None => 1,
        // ADR-0086: an on-demand node serves a downstream not launched yet; a
        // downstream already launched is not relaunched — Retry on it is the
        // explicit gesture, a cascade down to `End` is not.
        Some(_)
            if pipeline
                .nodes
                .iter()
                .any(|n| n.id == source_id && n.on_demand) =>
        {
            return None;
        }
        Some(ts) => {
            if graph_resolver::reaches(pipeline, target_id, source_id) {
                ts.iter + 1
            } else {
                return None;
            }
        }
    };

    let member_region = crate::domain::loop_region::bounded_region_for_member(pipeline, target_id);
    if let Some(region) = member_region {
        // Honour a live `set_region_max_iter` raise here too: on the YAML bound,
        // the region head would re-enter but its body node would refuse the new lap.
        let max = effective_region_max_iter(run_state, region, resolved_vars);
        if proposed > max {
            return None;
        }

        // A member→member forward edge lifts the target to the source's lap. Fire
        // only while the target is genuinely a lap behind: `reopen_run` re-fires
        // every completed member's edges in one pass, and a target that already
        // caught up would be re-spawned a lap ahead of the source's output.
        if region.members.iter().any(|m| m == source_id) {
            let source_iter = run_state.nodes.get(source_id).map(|n| n.iter).unwrap_or(1);
            if run_state
                .nodes
                .get(target_id)
                .is_some_and(|ts| ts.iter >= source_iter)
            {
                return None;
            }
        }
    }

    Some(proposed)
}
