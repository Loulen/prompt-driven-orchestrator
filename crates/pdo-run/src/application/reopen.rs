//! The re-open a targeted command embeds on a terminal Run (ADR-0049).

use anyhow::Result;

use crate::application::events::append_event;
use crate::application::events::load_events;
use crate::domain::{event, projection, state};
use crate::Runs;

/// Embed a re-open in a **targeted** human command (ADR-0049): retry / restart /
/// start / complete / inject on a terminal (or incident-parked) Run re-open it
/// **atomically** — one round-trip, no "resume then act" race, no re-fail window.
/// This is the human's own gesture reopening the Run; the runtime never reopens of
/// its own initiative (ADR-0009 amended).
///
/// If the Run needs re-opening (terminal & non-archived, or `AwaitingUser` on an
/// incident), it appends the same `reopen_run` `CommandIssued` the global reopen
/// uses — lifting the Run to `Running`, freezing satisfied `(node, iter)` and
/// dropping interrupted nodes so they re-drive (anti-#221) — then returns the
/// **re-projected** state so the caller's own guard runs against a live Run.
/// Otherwise (already live, or `Archived`) it returns the state unchanged and the
/// caller's guard decides. Returns `Err` only on a DB failure.
pub async fn embed_reopen_for_targeted_command(
    state: &Runs,
    run_id: &str,
    run_state: state::RunState,
) -> Result<state::RunState> {
    let needs_reopen = (run_state.status.is_terminal()
        && run_state.status != state::RunStatus::Archived)
        || (run_state.status == state::RunStatus::AwaitingUser
            && run_state.awaiting_reason.is_some()
            // #588: a declared wait carries a prose reason too, but it is not an
            // incident — nothing to reopen, the agent is simply asking.
            && !run_state.is_declared_wait());
    if !needs_reopen {
        return Ok(run_state);
    }
    let cmd_event = event::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::CommandIssued,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "command": "reopen_run" })),
    };
    append_event(state, &cmd_event).await?;
    let events = load_events(state, run_id).await?;
    Ok(projection::project(&events).unwrap_or(run_state))
}
