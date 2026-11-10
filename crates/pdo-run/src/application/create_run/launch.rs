//! A new Run, started: its ready nodes spawned and, when the instance says so,
//! its Pipeline Manager. A sandboxed Run prepares its container first, on a
//! detached task: the `201` must not block on a first-run `docker build`
//! (ADR-0023).

use std::path::Path;
use std::sync::Arc;

use crate::application::events::{emit_run_event, reload_run_state};
use crate::application::manager::spawn_manager_session;
use crate::application::node::prompt::RunNameHint;
use crate::application::sandbox::context_from_state;
use crate::application::sandbox_prep::{
    abandon_prep_if_run_is_terminal, fail_run_sandbox_prep, mark_sandbox_prep_ready,
};
use crate::application::scheduler::node_completion::spawn_ready_after_event;
use crate::domain::event::EventKind;
use crate::Runs;

/// Start the Run `run_id` just created in `worktree_dir`.
pub(super) async fn launch(
    state: &Arc<Runs>,
    run_id: &str,
    worktree_dir: &Path,
    sandboxed: bool,
    name_hint: RunNameHint,
    manager_enabled: bool,
) {
    if !sandboxed {
        // Host path — inline, no docker.
        spawn_ready_after_event(state, run_id).await;
        if manager_enabled {
            spawn_manager_session(state, run_id, worktree_dir, name_hint, false, None, None).await;
        }
    } else {
        // Eager fail-fast prep on a detached task: the 201 must not block on a
        // first-run `docker build` (ADR-0023). Image + container + staging are
        // guaranteed BEFORE the first node/manager spawn, and any Docker
        // unavailability is a `RunFailed` with ZERO host spawn — never a silent host
        // fallback for a sandboxed Run's work.
        let task_state = state.clone();
        let task_run_id = run_id.to_string();
        let task_worktree = worktree_dir.to_path_buf();
        tokio::spawn(async move {
            // Emitted at the HEAD of the detached task, before the long
            // `ensure_ready`, so the UI shows "préparation du sandbox…" instead of a
            // seemingly-stuck Run.
            emit_run_event(
                &task_state,
                &task_run_id,
                EventKind::SandboxPrepStarted,
                None,
            )
            .await;
            // A vanished/half-projected state → fail loud.
            let ctx = match reload_run_state(&task_state, &task_run_id).await {
                Some((_, rs)) => match context_from_state(&task_state, &rs).await {
                    Ok(ctx) => ctx,
                    Err(e) => {
                        fail_run_sandbox_prep(
                            &task_state,
                            &task_run_id,
                            &format!("sandbox prep failed: {e:#}"),
                        )
                        .await;
                        return;
                    }
                },
                None => {
                    fail_run_sandbox_prep(
                        &task_state,
                        &task_run_id,
                        "sandbox prep failed: run state disappeared before prep",
                    )
                    .await;
                    return;
                }
            };
            // `ensure_ready` blocks on docker build/probe, so it must run under
            // `spawn_blocking` — which also isolates its panic into a `JoinError`.
            // Up, AND reachable from inside: a container that cannot call the
            // daemon would leave every node running for ever.
            match tokio::task::spawn_blocking(move || ctx.ensure_ready_for_nodes()).await {
                Ok(Ok(())) => {
                    // The Run may have gone terminal WHILE the prep ran. Emitting
                    // `SandboxPrepReady` and spawning into a dead Run would leave a
                    // container nobody can exec into, so abandon instead. Idempotent —
                    // a later `resume_run` re-runs `ensure_ready`.
                    if abandon_prep_if_run_is_terminal(&task_state, &task_run_id).await {
                        return;
                    }
                    // Emitted just before the spawn so the prep banner clears exactly
                    // when work can start. This pair — event, then advance — is the
                    // single REPLAY point for every spawn deferred while the prep ran.
                    mark_sandbox_prep_ready(&task_state, &task_run_id).await;
                    spawn_ready_after_event(&task_state, &task_run_id).await;
                    if manager_enabled {
                        spawn_manager_session(
                            &task_state,
                            &task_run_id,
                            &task_worktree,
                            name_hint,
                            true,
                            None,
                            None,
                        )
                        .await;
                    }
                }
                Ok(Err(e)) => {
                    fail_run_sandbox_prep(
                        &task_state,
                        &task_run_id,
                        &format!("sandbox prep failed: {e:#}"),
                    )
                    .await;
                }
                Err(join_err) => {
                    fail_run_sandbox_prep(
                        &task_state,
                        &task_run_id,
                        &format!("sandbox prep panicked: {join_err}"),
                    )
                    .await;
                }
            }
        });
    }
}
