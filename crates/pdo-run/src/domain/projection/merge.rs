//! The merge-back signals, the retired resolver's history and the pipeline's
//! lint and edits.

use crate::domain::event::{Event, EventKind};
use crate::domain::state::*;

/// Merge-back signals: all informational. A conflict interrupts the node through
/// its own events; the retired resolver's events (ADR-0082) project nothing.
pub(super) fn apply_merge_event(_state: &mut RunState, event: &Event) {
    match event.kind {
        EventKind::MergeConflictDetected => {
            // Informational — the merge-back either resolves the conflict in
            // the node's favour (#503) or interrupts the node.
        }
        EventKind::MergeResolvedInNodeFavour => {
            // Informational (#503) — the completion carries on from here, so the
            // node's own `NodeCompleted` moves the state. Nothing to project: this
            // event exists so that a rewritten pipeline branch is never silent.
        }
        EventKind::MergeResolverStarted
        | EventKind::MergeResolverCompleted
        | EventKind::MergeResolverFailed => {
            // History of the retired merge resolver (ADR-0082). Archived Runs
            // still carry these events and must reopen, so they stay
            // deserialisable; nothing is projected — the cost and Stats read the
            // events themselves to keep the resolver's Infrastructure line.
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
