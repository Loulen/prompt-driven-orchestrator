//! The end of a sandboxed Run's preparation: ready (replay the deferred
//! spawns) or failed.

use tracing::{error, warn};

use crate::application::events::append_event;
use crate::application::events::emit_run_event;
use crate::application::events::reload_run_state;
use crate::domain::event;
use crate::Runs;

/// Record that a sandboxed Run's container is up — also the moment the spawns
/// deferred by [`state::RunState::sandbox_spawn_block`] become legal.
///
/// This event is the ONLY thing that lifts the spawn precondition, so every path that
/// makes the container real MUST go through here: the create-time prep task,
/// `resume_run` re-arming a terminal Run, and boot recovery reconciling a live one.
/// A Run whose prep completed outside the create path would otherwise keep a
/// `pending` projection for ever and defer every spawn for ever.
///
/// Idempotent. It deliberately does NOT advance the Run — each caller pairs it with
/// its own advance, and folding one in here would make this a scheduling entry point
/// (ADR-0009).
pub async fn mark_sandbox_prep_ready(state: &Runs, run_id: &str) {
    emit_run_event(state, run_id, event::EventKind::SandboxPrepReady, None).await;
}

/// Abandon a just-finished sandbox prep whose Run went terminal while it ran,
/// returning whether it abandoned. Without it, `container Created` can land tens of
/// seconds after `run_failed` and `sandbox_prep_ready` be emitted on a corpse.
///
/// Removes the container but deliberately KEEPS the staging: staging is purged by
/// `cleanup_run` at archive, and destroying it here would also destroy the
/// transcripts `merge_back` harvests. The in-flight filesystem walk is not
/// interrupted — the staging's `prepare` is a sync walk with no cancellation seam.
pub async fn abandon_prep_if_run_is_terminal(state: &Runs, run_id: &str) -> bool {
    let Some((_, run_state)) = reload_run_state(state, run_id).await else {
        // No projection at all: the Run was forgotten under the prep. Same verdict —
        // nothing may be spawned, and the container must not linger.
        warn!("Run {run_id}: sandbox prep finished for a run with no projected state — abandoning");
        remove_sandbox_container_best_effort(state, run_id);
        return true;
    };
    if !run_state.status.is_terminal() {
        return false;
    }
    warn!(
        "Run {run_id}: sandbox prep finished but the run is already {:?} — abandoning \
         (no prep-ready event, no spawn); removing its container",
        run_state.status
    );
    remove_sandbox_container_best_effort(state, run_id);
    true
}

/// `docker rm -f pdo-sbx-<run_id>` on a blocking thread, best-effort.
pub fn remove_sandbox_container_best_effort(state: &Runs, run_id: &str) {
    let docker_bin = state
        .docker_cmd_override
        .clone()
        .unwrap_or_else(|| "docker".to_string());
    let rid = run_id.to_string();
    tokio::task::spawn_blocking(move || {
        if let Err(e) =
            pdo_sandbox::adapters::local_runtime(Some(&docker_bin)).remove_container(&rid)
        {
            warn!(
                "Run {rid}: failed to remove the abandoned sandbox container (best-effort): {e:#}"
            );
        }
    });
}

/// Fail a sandboxed Run *loud* when its eager prep can't complete. A Run whose
/// container never came up must move terminal, never wedge `Running` with no live
/// node — and NEVER fall back to a host spawn.
pub async fn fail_run_sandbox_prep(state: &Runs, run_id: &str, reason: &str) {
    error!("Run {run_id}: sandbox prep failed — {reason}");
    let run_failed = event::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::RunFailed,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "reason": reason })),
    };
    if let Err(e) = append_event(state, &run_failed).await {
        error!("Run {run_id}: failed to append RunFailed after sandbox prep failure: {e}");
    }
}
