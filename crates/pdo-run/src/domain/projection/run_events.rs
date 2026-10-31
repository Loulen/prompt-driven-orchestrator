//! Run-level events: the Run starts, ends, pauses, resumes, is renamed, has
//! its repositories edited, is archived, prepares its sandbox.

use tracing::warn;

use super::reasons::{run_event_reason, run_event_reason_code};
use super::run_started::apply_run_started;
use crate::domain::event::{Event, EventKind};
use crate::domain::state::*;

pub(super) fn apply_run_event(state: &mut RunState, event: &Event) {
    match event.kind {
        EventKind::RunStarted => apply_run_started(state, event),
        EventKind::RunCompleted => {
            state.status = RunStatus::Completed;
            state.completed_at = Some(event.ts.clone());
            if let Some(ref mut end_node) = state.end_node {
                for port in &mut end_node.ports {
                    if port.status == "pending" {
                        port.status = "received".to_string();
                        port.fired_at = Some(event.ts.clone());
                    }
                }
            }
        }
        EventKind::RunFailed => {
            state.status = RunStatus::Failed;
            state.completed_at = Some(event.ts.clone());
            state.failure_reason = run_event_reason(event);
        }
        EventKind::RunSkipped => {
            // Graceful no-op (#245): terminal, like RunFailed/RunCompleted.
            // The run reached no `end` node (the selector short-circuited),
            // so end-node ports stay pending — only the run status reflects
            // "fired but nothing to do".
            state.status = RunStatus::Skipped;
            state.completed_at = Some(event.ts.clone());
            state.failure_reason = run_event_reason(event);
        }
        EventKind::RunHalted => {
            state.status = RunStatus::Halted;
            state.completed_at = Some(event.ts.clone());
            // A halt carries `message`, not `reason`. Surface it on the run too, so
            // all three non-green terminals answer "why?" through one field.
            state.failure_reason = event
                .payload
                .as_ref()
                .and_then(|p| p.get("message").or_else(|| p.get("reason")))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from);
            if let Some(ref mut end_node) = state.end_node {
                let reason = event
                    .payload
                    .as_ref()
                    .and_then(|p| p.get("message"))
                    .and_then(|v| v.as_str())
                    .map(String::from);
                for port in &mut end_node.ports {
                    port.status = "received".to_string();
                    port.reason = reason.clone();
                    port.fired_at = Some(event.ts.clone());
                }
            }
        }
        EventKind::RunPaused => {
            if state.status == RunStatus::Running || state.status == RunStatus::AwaitingUser {
                state.status = RunStatus::Paused;
            }
        }
        EventKind::RunResumed => {
            if state.status == RunStatus::Paused {
                state.status = RunStatus::Running;
            }
            // A Run being driven again must not still display last time's cause
            // (#503) — same rule `NodeStarted` applies to `NodeState::failure_reason`.
            state.failure_reason = None;
            // An incident park (ADR-0049) clears the same way: a resumed Run is
            // no longer waiting on that incident.
            state.awaiting_reason = None;
            state.awaiting_reason_code = None;
        }
        // The runtime gave up on driving the run forward for a non-deliberate
        // reason (ADR-0049): park it `AwaitingUser` with the reason, NEVER
        // `Failed`, and never set `completed_at` (it is not terminal). Inert on
        // an already-terminal run (#221): an active give-up racing a genuine
        // completion must not un-terminalize it. A live run (Running /
        // AwaitingUser / Paused) parks; a Paused run parks too so an operator
        // sees why it will not resume clean.
        EventKind::RunInterrupted => {
            if state.status.is_live() {
                state.status = RunStatus::AwaitingUser;
                state.awaiting_reason = run_event_reason(event);
                state.awaiting_reason_code = run_event_reason_code(event);
            }
        }
        EventKind::RunRenamed => {
            if let Some(ref payload) = event.payload {
                if let Some(new_name) = payload.get("name").and_then(|v| v.as_str()) {
                    if new_name.is_empty() {
                        state.name = None;
                    } else {
                        state.name = Some(new_name.to_string());
                    }
                }
            }
        }
        EventKind::RunReposEdited => {
            // #221 (double guard, reducer half): a terminal Run keeps the list it
            // was frozen with — a metadata event appended after a terminal event
            // (hand-crafted, replayed, or racing a completion) must NEVER re-open it
            // nor mutate its context. The handler already refuses the edit with a
            // 409, but the reducer is inert on its own so the log stays the single
            // source of truth even if an edit slipped in around the terminal event.
            if state.status.is_terminal() {
                return;
            }
            // The payload carries the whole re-frozen active list under
            // `target_repos` (mirror of the `RunStarted` arm above): overwrite
            // wholesale. Absent/unreadable degrades LOUDLY to "keep the previous
            // list" — never a panic (this applier runs inside `append_event`, before
            // the transition guard), and never a reset that would silently starve a
            // node reading a snapshot the projection forgot.
            if let Some(ref payload) = event.payload {
                match payload.get("target_repos") {
                    None => {}
                    Some(raw) => match serde_json::from_value::<Vec<RepoPin>>(raw.clone()) {
                        Ok(pins) => state.target_repos = pins,
                        Err(e) => warn!(
                            "run_repos_edited carries an unreadable `target_repos` value \
                             ({raw}): {e}; keeping the previous secondary list."
                        ),
                    },
                }
            }
        }
        EventKind::RunArchived => {
            state.status = RunStatus::Archived;
            // A Run archived while it ran ends there: its duration stops at
            // the archive, not at its last activity.
            state.completed_at.get_or_insert_with(|| event.ts.clone());
            state.start_node = None;
            state.end_node = None;
        }
        // #410: additive image-prep visibility. Non-terminal — `status` untouched,
        // only `sandbox_prep` moves. Emitted only for a sandboxed create (`full`/`minimal`);
        // `off`/historical runs never carry these, so the field stays `None`.
        EventKind::SandboxPrepStarted => {
            state.sandbox_prep = Some(SandboxPrepState::Pending);
        }
        EventKind::SandboxPrepReady => {
            state.sandbox_prep = Some(SandboxPrepState::Ready);
        }
        _ => {}
    }
}
