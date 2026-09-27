//! The run-level stall, reconciled: parked on its user, never failed
//! (ADR-0049).

use tracing::{error, warn};

use crate::application::events::append_event;
use crate::application::events::reload_run_state;
use crate::application::paths::effective_repo_root;
use crate::application::paths::resolve_run_pipeline_path;
use crate::application::scheduler::frontmatter::resolve_run_variables;
use crate::application::scheduler::node_completion::retry_waiting_nodes;
use crate::domain::run_stall::run_stall_reason;
use crate::domain::scheduler;
use crate::domain::scheduler::dispatcher as scheduler_dispatcher;
use crate::domain::{event, projection, state};
use crate::Runs;
use pdo_pipeline::domain::pipeline;

/// Seconds elapsed since the run's most recent event, or `None` when no event
/// carries a parseable RFC3339 timestamp. The most recent event has the *smallest*
/// age, hence the `min` — the name says `max` because it is the largest timestamp.
pub fn max_event_age_secs(events: &[event::Event]) -> Option<i64> {
    let now = chrono::Utc::now();
    events
        .iter()
        .filter_map(|e| chrono::DateTime::parse_from_rfc3339(&e.ts).ok())
        .map(|t| (now - t.with_timezone(&chrono::Utc)).num_seconds())
        .min()
}

/// Reconcile a run silently stalled at the **run level**: no live node, nothing the
/// scheduler can spawn, yet still `Running`. On a stall it appends a
/// `RunInterrupted`, parking the run `AwaitingUser` and **never `RunFailed`**
/// (ADR-0049: the runtime never declares forfeit of its own initiative).
pub async fn reconcile_run_level_stall(state: &Runs, run_id: &str) {
    let Some((events, run_state)) = reload_run_state(state, run_id).await else {
        return;
    };
    if run_state.status != state::RunStatus::Running {
        return;
    }

    let repo_root = effective_repo_root(state, &run_state);
    let pipeline_path = resolve_run_pipeline_path(&repo_root, run_id, &run_state.pipeline_name);
    let Ok(yaml) = std::fs::read_to_string(&pipeline_path) else {
        return;
    };
    let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
        return;
    };
    let pipeline = parse_result.pipeline;
    let resolved_vars = resolve_run_variables(&pipeline, &events);

    let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
    let loop_seed = scheduler::seed_pending_loops(&pipeline, &run_state, &resolved_vars);

    let idle_secs = max_event_age_secs(&events);

    let Some(reason) = run_stall_reason(&pipeline, &run_state, &ready, &loop_seed, idle_secs)
    else {
        return;
    };

    let run_interrupted = event::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::RunInterrupted,
        node_id: None,
        iter: None,
        // Every non-advancement carries a machine slug beside its prose.
        payload: Some(projection::interrupt_payload("run_stalled", reason.clone())),
    };
    // `RunInterrupted` is inert on an already-terminal run (its reducer gates on
    // `is_live`), so a run that finished since the snapshot is left untouched.
    if let Err(e) = append_event(state, &run_interrupted).await {
        error!("reconcile_run_level_stall: failed to park run {run_id}: {e}");
    } else {
        warn!("Run {run_id} reconciled to AwaitingUser (interrupted) — {reason}");
        // Freed slots: re-drive throttled `waiting` nodes in other runs.
        retry_waiting_nodes(state).await;
    }
}
