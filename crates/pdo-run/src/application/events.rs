//! A Run's log as the use cases read and write it: append (with the terminal
//! merge-back), emit the scheduler's actions, load and project.

use anyhow::Result;
use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tracing::error;

use crate::application::sandbox as run_sandbox;
use crate::domain::scheduler;
use crate::domain::{event, projection, state};
use crate::Runs;

/// Append an event through the run context's use case (guarded, stored, then
/// published on the bus), without the terminal merge-back of [`append_event`]:
/// what `spawn_node` appends is never terminal.
pub async fn append_event_with(run: &Runs, event: &event::Event) -> Result<()> {
    run.append_event().execute(event).await
}

/// [`append_event_with`], plus the terminal merge-back — the name every
/// in-process caller uses.
///
/// It is also the single chokepoint for the terminal `merge_back`: every emitter of
/// the four terminal Run events funnels through here. After a SUCCESSFUL terminal append, a sandboxed Run's staged
/// transcripts are merged into `~/.claude/projects/` so cost + stale-detection see
/// them once the staging is purged. The walk runs detached, so it never adds latency
/// or a failure mode to the terminal transition; it is idempotent, so a double-fire
/// is safe.
pub async fn append_event(state: &Runs, event: &event::Event) -> Result<()> {
    append_event_with(state, event).await?;
    if matches!(
        event.kind,
        event::EventKind::RunCompleted
            | event::EventKind::RunFailed
            | event::EventKind::RunSkipped
            | event::EventKind::RunHalted
    ) {
        run_sandbox::merge_back_best_effort(state, &event.run_id).await;
    }
    Ok(())
}

/// Whether a run_id has been tombstoned by forget (ADR-0024).
pub async fn run_is_forgotten(runs: &Runs, run_id: &str) -> Result<bool> {
    runs.store.is_forgotten(run_id).await
}

pub async fn emit_run_event(
    state: &Runs,
    run_id: &str,
    kind: event::EventKind,
    payload: Option<serde_json::Value>,
) {
    let event = event::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: pdo_kernel::clock::now_iso(),
        kind,
        node_id: None,
        iter: None,
        payload,
    };
    if let Err(e) = append_event(state, &event).await {
        error!("failed to append {:?}: {e}", event.kind);
    }
}

pub async fn emit_loop_action(state: &Runs, run_id: &str, action: &scheduler::SchedulerAction) {
    match action {
        scheduler::SchedulerAction::LoopIterStarted {
            loop_node_id,
            iter,
            max_iter,
        } => {
            emit_run_event(
                state,
                run_id,
                event::EventKind::LoopIterStarted,
                Some(serde_json::json!({
                    "loop_node_id": loop_node_id,
                    "iter": iter,
                    "max_iter": max_iter,
                })),
            )
            .await;
        }
        scheduler::SchedulerAction::LoopBreakReceived { loop_node_id } => {
            emit_run_event(
                state,
                run_id,
                event::EventKind::LoopBreakReceived,
                Some(serde_json::json!({
                    "loop_node_id": loop_node_id,
                })),
            )
            .await;
        }
        scheduler::SchedulerAction::LoopMaxReached {
            loop_node_id,
            max_iter,
        } => {
            emit_run_event(
                state,
                run_id,
                event::EventKind::LoopMaxReached,
                Some(serde_json::json!({
                    "loop_node_id": loop_node_id,
                    "max_iter": max_iter,
                })),
            )
            .await;
        }
        scheduler::SchedulerAction::LoopDone { loop_node_id } => {
            emit_run_event(
                state,
                run_id,
                event::EventKind::LoopDone,
                Some(serde_json::json!({
                    "loop_node_id": loop_node_id,
                })),
            )
            .await;
        }
        _ => {}
    }
}

pub async fn emit_collection_action(
    state: &Runs,
    run_id: &str,
    action: &scheduler::SchedulerAction,
) {
    match action {
        scheduler::SchedulerAction::CollectionStarted { .. } => {
            emit_run_event(
                state,
                run_id,
                event::EventKind::CollectionStarted,
                scheduler::collection_started_payload(action),
            )
            .await;
        }
        scheduler::SchedulerAction::CollectionEmpty { region_id } => {
            emit_run_event(
                state,
                run_id,
                event::EventKind::CollectionEmpty,
                Some(serde_json::json!({
                    "region_id": region_id,
                })),
            )
            .await;
        }
        scheduler::SchedulerAction::CollectionDone { region_id } => {
            emit_run_event(
                state,
                run_id,
                event::EventKind::CollectionDone,
                Some(serde_json::json!({
                    "region_id": region_id,
                })),
            )
            .await;
        }
        _ => {}
    }
}

/// Every event of `run_id`, in append order.
pub async fn load_events(runs: &Runs, run_id: &str) -> Result<Vec<event::Event>> {
    runs.store.events(run_id).await
}

/// The id of every Run of the log, newest first.
pub async fn load_all_run_ids(runs: &Runs) -> Result<Vec<String>> {
    runs.store.run_ids().await
}

/// Reload events and re-project the run state; `None` (logged) when the log
/// cannot be read or holds no started Run.
pub async fn reload_run_state_with(
    runs: &Runs,
    run_id: &str,
) -> Option<(Vec<event::Event>, state::RunState)> {
    let events = match load_events(runs, run_id).await {
        Ok(e) => e,
        Err(e) => {
            error!("reload_run_state: failed to load events for {run_id}: {e}");
            return None;
        }
    };
    let run_state = projection::project(&events)?;
    Some((events, run_state))
}

/// Reload events and re-project the run state.
///
/// Use after appending events inside a multi-pass dispatch so the next pass
/// observes the projection it would produce on the next tick. Without this,
/// flags like `loop_states[id].break_received` stay stale within the function
/// and downstream evaluators take decisions on stale state.
pub async fn reload_run_state(
    state: &Runs,
    run_id: &str,
) -> Option<(Vec<event::Event>, state::RunState)> {
    reload_run_state_with(state, run_id).await
}

/// Load a Run's events and project its state, or hand back the `Response` the
/// caller must return: `500 text/plain` on a DB failure, `404 text/plain` when
/// the projection is `None`.
///
/// The 404-preserving half of the pair it sits next to. `reload_run_state_with`
/// (just above) collapses both failure modes into an `Option` — logging the
/// `Err` and dropping the Err/None distinction — because its callers only ask
/// "is there state to advance?". This one keeps `Err` and `None` apart and maps
/// each to its own status, because its callers are HTTP surfaces that owe the
/// client a code. Do NOT rewrite either as a wrapper of the other: the merge
/// would throw away exactly the information each keeps for its own callers.
///
/// `None` means one of two things, both "there is no such Run": an empty log,
/// or a log carrying no `RunStarted` — an invalid fragment, e.g. an event
/// appended after a forget (`event_log.rs`, #328). Either way `404` is the
/// right answer; a Run that exists always has its `RunStarted` first.
///
/// `Box<Response>` rather than `Response`: `size_of::<Response>()` is 128, exactly
/// the `clippy::result_large_err` threshold, and CI runs `-D warnings`.
///
/// **Opt-in per call site, never a global preamble.** Several load-and-project
/// blocks deliberately do NOT route here, and folding them in would silently change
/// a wire contract: the ones wanting the raw `Option<RunState>` because their guard
/// maps `None -> Allow`; `inject_artifact`, designed to work BEFORE the Run exists;
/// the sandbox probes, which treat `Err` and `None` alike as "no container to kill";
/// and the quasi-twins answering a different shape (`404`-in-JSON,
/// `get_run_events`'s `200 []` for an unknown run, the bulk-projecting filter loops).
///
/// Do NOT absorb the `410` tombstone check either: ADR-0024 §3 places that gate at
/// exactly three boundaries, and folding it in would flip every GET reaching here
/// from `404` to `410`.
pub async fn load_projected(
    state: &Runs,
    run_id: &str,
) -> Result<(Vec<event::Event>, state::RunState), Box<Response>> {
    let events = match load_events(state, run_id).await {
        Ok(e) => e,
        Err(e) => {
            // Error bodies are JSON, so a front `.catch(() => null)` on
            // `resp.json()` can read the reason instead of swallowing text.
            return Err(Box::new(
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("error: {e}") })),
                )
                    .into_response(),
            ));
        }
    };
    match projection::project(&events) {
        Some(run_state) => Ok((events, run_state)),
        None => Err(Box::new(
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "run not found" })),
            )
                .into_response(),
        )),
    }
}
