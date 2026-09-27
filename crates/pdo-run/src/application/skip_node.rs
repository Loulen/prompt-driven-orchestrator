//! `SkipNode`: the agent's graceful no-op (`pdo skip`) — the node completed,
//! the Run ended `skipped`.

use std::sync::Arc;

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tracing::{error, info};

use crate::application::complete_node::completion_head_gate;
use crate::application::events::append_event;
use crate::application::events::load_projected;
use crate::application::node::reap::reap_node_session;
use crate::application::operations::on_node;
use crate::application::paths::effective_repo_root;
use crate::application::scheduler::advance as run_advance;
use crate::application::scheduler::node_completion::retry_waiting_nodes;
use crate::application::supervision::detach_terminal_tail;
use crate::domain::event;
use crate::Runs;
use pdo_kernel::Actor;

/// `SkipNode`: the node's Agent (`pdo skip`) has legitimately nothing to do.
#[derive(Debug, Clone)]
pub struct SkipNode {
    pub run_id: String,
    pub node_id: String,
    pub iter: Option<i64>,
    pub reason: String,
}

impl Runs {
    /// Graceful no-op. A node that legitimately has nothing to do calls
    /// `pdo skip --reason …` instead of `pdo fail`. The node is recorded as completed —
    /// it ran and reached a decision — and the run ends in the distinct terminal state
    /// `Skipped`, short-circuiting downstream so no bogus input propagates and the
    /// failure signal stays honest.
    pub async fn skip_node(self: &Arc<Self>, actor: &Actor, command: SkipNode) -> Response {
        let SkipNode {
            run_id,
            node_id,
            iter,
            reason,
        } = command;
        if let Err(denied) = self.authorize(actor, &on_node("skip", &run_id, &node_id)) {
            return denied.into_response();
        }
        let state = self;

        let iter = iter.unwrap_or(1);

        let (_, pre_run_state) = match load_projected(state, &run_id).await {
            Ok(t) => t,
            Err(resp) => return *resp,
        };

        // Same guard as a completion: the skip terminalizes the node as Completed, so a
        // duplicate skip must be rejected/no-op'd exactly like `node_done`, never
        // double-appended.
        if let Some(stop) = completion_head_gate(
            run_advance::evaluate_completion_head(Some(&pre_run_state), &run_id, &node_id, iter),
            "node_skip",
            &run_id,
            &node_id,
            iter,
        ) {
            return stop.into_response();
        }

        // A graceful no-op produced no outputs and made no code changes, so — unlike
        // `node_done` — the sub-worktree merge and output validation are deliberately
        // skipped: there is nothing to merge or validate.
        let node_completed = event::Event {
            id: None,
            run_id: run_id.clone(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event::EventKind::NodeCompleted,
            node_id: Some(node_id.clone()),
            iter: Some(iter),
            payload: Some(serde_json::json!({ "skipped": true, "reason": reason })),
        };
        if let Err(e) = append_event(state, &node_completed).await {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }

        // Same self-cancellation window as `node_done`: the reap kills `pdo skip`'s own
        // session, so inline the `RunSkipped` append below could be silently dropped.
        let repo_root = effective_repo_root(state, &pre_run_state);
        let tail_state = state.clone();
        let tail_run = run_id.clone();
        let tail_node = node_id.clone();
        let tail_reason = reason.clone();
        let tail_sandbox = !pre_run_state.sandbox.is_off();
        detach_terminal_tail("node_skip", state.clone(), run_id, node_id, async move {
            reap_node_session(
                &tail_state,
                &repo_root,
                &tail_run,
                &tail_node,
                iter,
                tail_sandbox,
            );

            // Distinct from RunFailed, and it short-circuits downstream:
            // `spawn_ready_after_event` only spawns for a Running/AwaitingUser run.
            let run_skipped = event::Event {
                id: None,
                run_id: tail_run.clone(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event::EventKind::RunSkipped,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "reason": tail_reason })),
            };
            if let Err(e) = append_event(&tail_state, &run_skipped).await {
                error!("failed to append run_skipped: {e}");
            }

            // The run ended, so its other sessions will be reaped and free slots.
            retry_waiting_nodes(&tail_state).await;

            info!("Node {tail_node} skipped (graceful no-op) in run {tail_run}: {tail_reason}");
        });
        (
            StatusCode::OK,
            Json(serde_json::json!({ "ok": true, "skipped": true, "reason": reason })),
        )
            .into_response()
    }
}
