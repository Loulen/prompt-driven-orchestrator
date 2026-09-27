//! The merge resolver's lifecycle and the pipeline's lint and edits.

use crate::domain::event::{Event, EventKind};
use crate::domain::state::*;

/// Merge-resolver lifecycle: the conflict signal is informational; the resolver
/// then runs and either completes or fails, tracked in `merge_resolver`.
pub(super) fn apply_merge_event(state: &mut RunState, event: &Event) {
    match event.kind {
        EventKind::MergeConflictDetected => {
            // Informational — the run either spawns a resolver, resolves the
            // conflict in the node's favour (#503) or fails.
        }
        EventKind::MergeResolvedInNodeFavour => {
            // Informational (#503) — the completion carries on from here, so the
            // node's own `NodeCompleted` moves the state. Nothing to project: this
            // event exists so that a rewritten pipeline branch is never silent.
        }
        EventKind::MergeResolverStarted => {
            if let Some(ref payload) = event.payload {
                let conflicting_node_id = payload
                    .get("conflicting_node_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let iter = payload.get("iter").and_then(|v| v.as_i64()).unwrap_or(1);
                let session_name = payload
                    .get("session_name")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                state.merge_resolver = Some(MergeResolverInfo {
                    status: NodeStatus::Running,
                    conflicting_node_id,
                    iter,
                    session_name,
                    started_at: Some(event.ts.clone()),
                    completed_at: None,
                    failure_reason: None,
                });
            }
        }
        EventKind::MergeResolverCompleted => {
            if let Some(ref mut mr) = state.merge_resolver {
                mr.status = NodeStatus::Completed;
                mr.completed_at = Some(event.ts.clone());
            }
        }
        EventKind::MergeResolverFailed => {
            if let Some(ref mut mr) = state.merge_resolver {
                mr.status = NodeStatus::Failed;
                mr.completed_at = Some(event.ts.clone());
                if let Some(ref payload) = event.payload {
                    mr.failure_reason = payload
                        .get("reason")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                }
            }
        }
        _ => {}
    }
}

/// Pipeline-file events. Both are passive signals that intentionally make NO
/// change to the projected state — `PipelineLint` is informational, and
/// `PipelineModified` must NEVER un-terminalize a run (#221, see below). Kept as
/// its own applier so the load-bearing #221 rationale lives next to the no-op.
pub(super) fn apply_pipeline_event(_state: &mut RunState, event: &Event) {
    match event.kind {
        EventKind::PipelineLint => {
            // Informational — records lint diagnostics for the pipeline
        }
        EventKind::PipelineModified => {
            // Node_defs/edges are re-parsed from the file at scheduling time
            // (`spawn_ready_after_event`), so a live run picks up newly-added nodes
            // on the next tick with no status change here.
            //
            // Don't un-terminalize the run (#221): a `PipelineModified` is a passive
            // signal that a stray or foreign file write can emit, even for a node
            // outside this run's DAG. Reopening a terminal run here leaves it
            // phantom-`running` forever (no reliable re-completion path), holds its
            // manager session and worktree, makes overlap-`skip` triggers skip every
            // subsequent fire, and lets a later `resume_run` re-spawn satisfied loops.
            // Picking up newly-added work is the explicit `resume_run`, not a side
            // effect of the file watcher.
        }
        _ => {}
    }
}
