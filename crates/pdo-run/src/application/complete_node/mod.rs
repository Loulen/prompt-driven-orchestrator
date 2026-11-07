//! `CompleteNode`: the completion of a node iteration — the use case the
//! agent's `pdo complete` reaches, and the shared completion body the liveness
//! sweep's turn-end auto-completion takes as a transition.
//!
//! - [`outcome`]: what an attempt to complete answers (a completion, a
//!   no-op, a refusal) and where it came from.
//! - [`delivery`]: delivering a node's work into the Run's branch.

mod delivery;
mod outcome;

use std::sync::Arc;

use axum::response::{IntoResponse, Response};
use pdo_kernel::Actor;
use tracing::info;

pub use delivery::{deliver_node_run, secondary_repos_dirtied_refusal};
pub use outcome::*;

use crate::application::events::append_event;
use crate::application::events::load_events;
use crate::application::events::run_is_forgotten;
use crate::application::gesture;
use crate::application::node::launch::node_run_isolation;
use crate::application::node::output_validation::check_output_validation_with_retry;
use crate::application::node::reap::reap_dead_node_after_run_failure;
use crate::application::node::reap::reap_node_session;
use crate::application::operations::on_node;
use crate::application::paths::effective_repo_root;
use crate::application::paths::resolve_run_pipeline_path;
use crate::application::scheduler::advance as run_advance;
use crate::application::supervision::detach_terminal_tail;
use crate::domain::completion_refusal;
use crate::domain::{event, projection, state};
use crate::Runs;
use pdo_workspace::adapters::git::worktrees::worktree_dir_for_run;

/// `CompleteNode`: an actor — the node's Agent through `pdo complete`, or its
/// `Stop` hook on a turn end — asks to complete one iteration.
#[derive(Debug, Clone)]
pub struct CompleteNode {
    pub run_id: String,
    pub node_id: String,
    pub iter: i64,
    /// Runtime-initiated on turn end (the injected `Stop` hook), not a typed
    /// `pdo complete`: recorded as `NodeAutoCompleted`, source `StopHook`.
    pub auto: bool,
}

impl Runs {
    /// Asks the Autorisation d'opération, then runs the shared completion body
    /// [`complete_node_iteration`] — which the liveness sweep's turn-end
    /// auto-completion takes too, as a transition, without asking.
    ///
    /// The body lives in that shared function since #469 §3: the forgotten-run
    /// refusal (#328), the completion guard (#212/#354), the sub-worktree
    /// commit+merge, the non-isolated cleanliness check, output validation, the
    /// terminal append, then the detached reap+advance tail (#304/ADR-0023).
    pub async fn complete_node(self: &Arc<Self>, actor: &Actor, command: CompleteNode) -> Response {
        let run_id = command.run_id.clone();
        gesture::by(actor, &run_id, self.complete_node_gesture(actor, command)).await
    }

    async fn complete_node_gesture(
        self: &Arc<Self>,
        actor: &Actor,
        command: CompleteNode,
    ) -> Response {
        let CompleteNode {
            run_id,
            node_id,
            iter,
            auto,
        } = command;
        if let Err(denied) = self
            .authorize(actor, &on_node("complete", &run_id, &node_id))
            .await
        {
            return denied.into_response();
        }

        let source = if auto {
            CompletionSource::StopHook
        } else {
            CompletionSource::Explicit
        };
        complete_node_iteration(self, run_id, node_id, iter, source)
            .await
            .into_response()
    }
}

/// The shared node-completion body, for the agent's completion and the
/// liveness sweep's turn-end auto-completion alike.
pub async fn complete_node_iteration(
    state: &Arc<Runs>,
    run_id: String,
    node_id: String,
    iter: i64,
    source: CompletionSource,
) -> CompletionAttempt {
    let caller = source.label();

    // Refuse completions for a forgotten run BEFORE any side effect — the
    // sub-worktree merge in particular (ADR-0024).
    match run_is_forgotten(state, &run_id).await {
        Ok(true) => {
            // ADR-0024 §3 keeps its `410`: "never a 2xx" is not "always a 409".
            return CompletionAttempt::refused(
                completion_refusal::CompletionRefusal::RunForgotten {
                    run_id: run_id.clone(),
                },
            );
        }
        Ok(false) => {}
        Err(e) => {
            return CompletionAttempt::refused(completion_refusal::CompletionRefusal::Internal {
                error: format!("forgotten-run check failed: {e}"),
            });
        }
    }

    // A node's tmux session is snapshot-then-killed *immediately* at its terminal
    // transition (the reaps below), so post-mortem preview survives via the persisted
    // snapshot, not a lingering session. The reaper TTL is only an escaped-session
    // backstop. `cleanup_run` removes worktree/branches on demand; the runtime never
    // deletes those on its own (ADR-0012a).

    let events = match load_events(state, &run_id).await {
        Ok(e) => e,
        Err(e) => {
            return CompletionAttempt::refused(completion_refusal::CompletionRefusal::Internal {
                error: format!("failed to load events: {e}"),
            });
        }
    };

    let pre_run_state = match projection::project(&events) {
        Some(s) => s,
        None => {
            return CompletionAttempt::refused(completion_refusal::CompletionRefusal::RunNotFound);
        }
    };

    let repo_root = effective_repo_root(state, &pre_run_state);
    let worktree_dir = worktree_dir_for_run(&repo_root, &run_id);

    // Transition guard (#212, #354): validate the completion against the
    // projected state BEFORE any side effect (sub-worktree merge, non-isolated
    // cleanliness check, output validation, downstream dispatch). A duplicate
    // completion is a no-op — it must not merge again nor re-trigger downstream
    // spawns. The pure decision lives in the shared head.
    //
    // This gate admits only `Running`/`AwaitingUser`/`Failed`, so a latched-`Stale`
    // node's eventual `pdo complete` would be refused with its work already on disk.
    // Nothing marks a live node `Stale` any more, so the agent of a long tool call is
    // still `Running` here and its late completion is accepted.
    let head = run_advance::evaluate_completion_head(Some(&pre_run_state), &run_id, &node_id, iter);
    // The head's two stops map to two *different* variants: a legal duplicate stays a
    // `2xx` no-op, a reject is a refusal.
    if let Some(stop) = completion_head_gate(head, caller, &run_id, &node_id, iter) {
        return match stop {
            CompletionHeadStop::NoOp { reason } => CompletionAttempt::NoOp { reason },
            CompletionHeadStop::Refused { refusal } => CompletionAttempt::refused(refusal),
        };
    }

    if let Some(refusal) = interactive_completion_refusal(&pre_run_state, &node_id, iter) {
        // #588 / ADR-0069 §1: the agent is done and genuinely waits for the
        // click — the daemon declares the wait for it, so the run turns amber
        // with the reason on the banner (A3). Best-effort; the refusal stands.
        state
            .waits
            .on_completion_not_released(&run_id, &node_id, iter)
            .await;
        return CompletionAttempt::refused(refusal);
    }

    // Liaison forte nœud parent ↔ enfants (#724 / ADR-0064): ANY node whose
    // session created child runs (agent or `script`, Orchestrator toggle or not)
    // does not terminate while those children are non-terminal. The gate reads
    // the real children, never the toggle, and holds every source that reaches
    // this body (explicit, Stop hook, turn end, end of a script). It sits after
    // the transition guard but BEFORE any effect (delivery, validation): a
    // refusal leaves the node alive and its session intact — and, with a
    // `failed` child, parks the node `AwaitingUser` for the user's arbitration
    // (retry the child, or force through `mark_node_done`, which is
    // deliberately NOT gated).
    if let Some(refusal) =
        run_advance::parent_node_binding_refusal(state, &events, &run_id, &node_id, iter).await
    {
        return CompletionAttempt::refused(refusal);
    }

    // Must sit here: after the transition gate but BEFORE any merge or terminal
    // event. Non-terminal by design — the node stays alive, so once the agent reverts
    // the tracked change and re-completes, the guard passes.
    if let Some(refusal) = secondary_repos_dirtied_refusal(&repo_root, &run_id, &pre_run_state) {
        return CompletionAttempt::refused(refusal);
    }

    // #654 / ADR-0060: the ONE delivery every completion path goes through, run
    // before the terminal event and therefore before any downstream spawn.
    if let Some(refusal) = deliver_node_run(
        state,
        &events,
        &pre_run_state,
        &repo_root,
        &worktree_dir,
        &run_id,
        &node_id,
        iter,
    )
    .await
    {
        return CompletionAttempt::refused(refusal);
    }

    let pipeline_path =
        resolve_run_pipeline_path(&repo_root, &run_id, &pre_run_state.pipeline_name);
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
    // Reached on the auto path too, and NOT redundant with the sweep's own outputs
    // guard: that guard read the artifacts a moment earlier, this one is the authority
    // and owns the frontmatter corrective loop. If the two ever disagree the answer is
    // this one's.
    if let Some(refusal) = check_output_validation_with_retry(
        state,
        &pipeline_path,
        &node_id,
        iter,
        &artifacts_dir,
        &run_id,
        &pre_run_state,
    )
    .await
    {
        return CompletionAttempt::refused(refusal);
    }

    let event = event::Event {
        id: None,
        run_id: run_id.clone(),
        ts: pdo_kernel::clock::now_iso(),
        // `NodeAutoCompleted` on the sweep path — same projection, same guard, but
        // the log must say the completion was automatic. Deliberately the event KIND
        // and not a `source` field in the payload. The binding's own verdict
        // (#724) signs its payload instead: the log says WHO completed and why.
        kind: source.event_kind(),
        node_id: Some(node_id.clone()),
        iter: Some(iter),
        payload: source.payload(),
    };

    if let Err(e) = append_event(state, &event).await {
        return CompletionAttempt::refused(completion_refusal::CompletionRefusal::AppendFailed {
            error: e.to_string(),
        });
    }

    // Detached tail (ADR-0023): the reap below kills the very tmux session
    // `pdo complete` runs in, closing this request's client socket — inline, hyper
    // would cancel this future at its next `.await` and silently drop the advance.
    // The 2xx means "terminal event recorded, advance scheduled", not "advanced".
    //
    // On the auto path there is no client socket to lose, but detaching it too keeps
    // the two paths one path.
    let tail_state = state.clone();
    let tail_run = run_id.clone();
    let tail_node = node_id.clone();
    // A `bool`, not the mode: the mode owns a String, and this value is captured into
    // a detached `async move` where a reference cannot live.
    let tail_sandbox = !pre_run_state.sandbox.is_off();
    detach_terminal_tail(caller, state.clone(), run_id, node_id, async move {
        // Snapshot the pane then kill the session, so a completed node never holds a
        // live session toward the tmux-collapse point.
        reap_node_session(
            &tail_state,
            &repo_root,
            &tail_run,
            &tail_node,
            iter,
            tail_sandbox,
        );

        // Fire this node's edges, advance the run, re-drive throttled waiters, then
        // the single completion gate.
        match run_advance::complete_node(
            &tail_state,
            &tail_run,
            &tail_node,
            run_advance::CompletionOrder::CompletionFirst,
            false,
        )
        .await
        {
            run_advance::CompletionOutcome::Halted => info!("Run {tail_run} halted"),
            _ => info!("Node {tail_node} completed in run {tail_run} ({caller})"),
        }
    });
    CompletionAttempt::Completed
}

pub fn interactive_completion_refusal(
    run_state: &state::RunState,
    node_id: &str,
    iter: i64,
) -> Option<completion_refusal::CompletionRefusal> {
    let attempt = run_state
        .nodes
        .get(node_id)?
        .iterations
        .iter()
        .find(|attempt| attempt.iter == iter)?;
    (attempt.interactive && !attempt.completion_released)
        .then_some(completion_refusal::CompletionRefusal::CompletionNotReleased)
}
