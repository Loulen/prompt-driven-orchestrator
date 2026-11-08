//! `CommandIssued`: what the Pipeline Manager asked of the Run.

use crate::domain::event::Event;
use crate::domain::state::*;

/// `CommandIssued`: the projection-relevant manager/operator commands. A command
/// dispatcher by nature — `resume_run` re-opens a terminal run and `end_region`
/// closes a loop region — so the whole event is kept in one applier even though
/// it touches both run status and `loop_states`.
pub(super) fn apply_command_event(state: &mut RunState, event: &Event) {
    if let Some(ref payload) = event.payload {
        let cmd = payload.get("command").and_then(|v| v.as_str());
        // Re-opening a terminal Run (ADR-0049 / ADR-0032 amended): `terminal ≠
        // locked`. A human gesture — the global `reopen_run` (Play button), or a
        // targeted command that embeds its own re-open — lifts ANY terminal Run
        // back to `Running` by a safe re-projection: the satisfied `(node, iter)`
        // stay `Completed` (the scheduler's dedup refuses to re-spawn them,
        // anti-#221), only the unsatisfied work runs. `resume_run` is the
        // historical name of the same gesture, kept so every replayed log still
        // re-opens (append-only). Both clear the previous terminal/incident
        // reason — a Run being driven again must not still show last time's
        // cause (#503) — while the terminal *label* stays in the event log.
        // `Completed`/`Skipped` are in the set on purpose: a finished Run can pick
        // up a newly-added node (FP #6).
        if matches!(cmd, Some("reopen_run") | Some("resume_run"))
            && matches!(
                state.status,
                RunStatus::Halted
                    | RunStatus::Failed
                    | RunStatus::Completed
                    | RunStatus::Skipped
                    | RunStatus::AwaitingUser
            )
        {
            state.status = RunStatus::Running;
            state.completed_at = None;
            state.failure_reason = None;
            state.awaiting_reason = None;
            state.awaiting_reason_code = None;
            state.awaiting_region_id = None;
            // Re-drive the interrupted work (ADR-0049 default: restart with the
            // partial artefacts, which persist in the sub-worktree). Drop each
            // `Interrupted` node from the projection — exactly like
            // `NodeInvalidated` — so the scheduler re-spawns it fresh (its
            // sub-worktree is reused, feeding the partial work) instead of
            // leaving it parked; without this, [`finalize`] would re-derive
            // `AwaitingUser` from the still-interrupted node and the re-open
            // would not stick. The satisfied `Completed` nodes are untouched, so
            // the scheduler never re-spawns them (anti-#221). The event log keeps
            // every `NodeInterrupted` — this only rewinds the *projection*.
            state
                .nodes
                .retain(|_, n| n.status != NodeStatus::Interrupted);
        }
        // #199: `end_region` CLOSES the region — the projection
        // marks its loop state done so the scheduler's region
        // engine routes the exit instead of starting a phantom lap.
        // A region still on lap 1 has no loop state yet (the entry
        // appears when the first re-entry fires): create it closed,
        // so an early `end_region` is never lost. `max_iter` is
        // unknown to the projection (it lives in the pipeline) and
        // unused once the region is done.
        if cmd == Some("end_region") {
            if let Some(region_id) = payload.get("region_id").and_then(|v| v.as_str()) {
                state
                    .loop_states
                    .entry(region_id.to_string())
                    .or_insert_with(|| LoopState {
                        loop_node_id: region_id.to_string(),
                        current_iter: 1,
                        max_iter: 0,
                        break_received: false,
                        done: false,
                    })
                    .done = true;
            }
        }
        // #600 / ADR-0011: `set_region_max_iter` raises a bounded region's cap in
        // flight. Absolute and last-write-wins (unlike `bump_region`, which is
        // additive) — the operator names the total number of laps they want, not a
        // delta. The scheduler reads this override in place of the region's declared
        // `max_iter`, so it lifts a literal cap and a `$var` cap the same way. A
        // non-positive cap is ignored (a region is never made zero-lap by a stray
        // command); the source of truth stays the append-only log, so the raise
        // holds across a reopen re-projection.
        if cmd == Some("set_region_max_iter") {
            if let (Some(region_id), Some(n)) = (
                payload.get("region_id").and_then(|v| v.as_str()),
                payload.get("max_iter").and_then(|v| v.as_i64()),
            ) {
                if n > 0 {
                    state
                        .region_max_iter_overrides
                        .insert(region_id.to_string(), n);
                }
            }
        }
        // `set_region_max_cost` edits a bounded region's cost cap in flight
        // (ADR draft plafond-de-cout-de-boucle). Absolute and last-write-wins
        // like `set_region_max_iter`, but with NO floor (a cap under the spend
        // is legal: the loop stops at the next re-entry) and `null` meaning
        // « remove the cap ». A non-positive or non-numeric value is ignored
        // (the handler refuses it before appending).
        if cmd == Some("set_region_max_cost") {
            if let Some(region_id) = payload.get("region_id").and_then(|v| v.as_str()) {
                match payload.get("max_cost_usd") {
                    Some(serde_json::Value::Null) => {
                        state
                            .region_max_cost_overrides
                            .insert(region_id.to_string(), None);
                    }
                    Some(v) => {
                        if let Some(usd) = v.as_f64().filter(|u| u.is_finite() && *u > 0.0) {
                            state
                                .region_max_cost_overrides
                                .insert(region_id.to_string(), Some(usd));
                        }
                    }
                    None => {}
                }
            }
        }
        // #600 / ADR-0011: `force_route` records an explicit exit from a node or a
        // region to a target, short-circuiting the source's `when:` edges. Folded
        // per source (last-write-wins) so the effect is deterministic on every
        // re-projection — the forced route is NOT re-decided by `when:` on the next
        // lap or after a reopen (FP #8). Both endpoints are validated against the
        // pipeline snapshot before the command is appended (the handler), so the
        // projection trusts the payload.
        if cmd == Some("force_route") {
            if let (Some(from), Some(target)) = (
                payload.get("from").and_then(|v| v.as_str()),
                payload.get("target").and_then(|v| v.as_str()),
            ) {
                state
                    .forced_routes
                    .insert(from.to_string(), target.to_string());
            }
        }
    }
}
