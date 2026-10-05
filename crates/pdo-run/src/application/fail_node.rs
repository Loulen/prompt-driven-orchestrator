//! `FailNode`: the agent's `pdo fail` — the node failed, the Run failed or
//! parked on its user per `auto_fail` (ADR-0049).

use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tracing::{error, info};

use crate::application::events::append_event;
use crate::application::events::load_events;
use crate::application::events::reload_run_state;
use crate::application::node::reap::reap_node_session;
use crate::application::operations::on_node;
use crate::application::paths::effective_repo_root;
use crate::application::paths::resolve_run_pipeline_path;
use crate::application::scheduler::node_completion::retry_waiting_nodes;
use crate::application::supervision::detach_terminal_tail;
use crate::domain::auto_fail;
use crate::domain::{event, projection, state};
use crate::Runs;
use pdo_kernel::Actor;
use pdo_pipeline::domain::pipeline;

/// Resolve a node's effective `auto_fail` (ADR-0049) — gather all four tiers and
/// hand them to the pure [`auto_fail::resolve_auto_fail`].
///
/// - **node**: the node's `auto_fail:` in the run's (live-snapshot) pipeline;
/// - **run**: the frozen `RunState::auto_fail`;
/// - **project**: the Projet owning the run's primary repo (verbatim path);
/// - **instance**: `stored → env → false`.
///
/// Best-effort per tier: a tier that cannot be read (pipeline unparsable, DB
/// error) is treated as "states no preference" (`None`) / the floor, so the
/// resolution never fails the request — it only decides whether an agent
/// `pdo fail` terminalises or parks.
pub async fn resolve_run_auto_fail(
    state: &Runs,
    run_state: &state::RunState,
    node_id: &str,
) -> bool {
    let repo_root = effective_repo_root(state, run_state);

    // Node tier: parse the run-scoped pipeline and read this node's `auto_fail`.
    let node = {
        let pipeline_path =
            resolve_run_pipeline_path(&repo_root, &run_state.run_id, &run_state.pipeline_name);
        std::fs::read_to_string(&pipeline_path)
            .ok()
            .and_then(|yaml| pipeline::parse_pipeline(&yaml).ok())
            .and_then(|parsed| {
                parsed
                    .pipeline
                    .nodes
                    .iter()
                    .find(|n| n.id == node_id)
                    .and_then(|n| n.auto_fail)
            })
    };

    // Project tier: the Projet owning the run's primary repo (compared verbatim,
    // like the harness tier). Falls back to the daemon repo root when the run
    // carries no explicit target (ADR-0033 read-side resolution).
    let primary_repo = run_state
        .target_repo
        .clone()
        .unwrap_or_else(|| state.repo_root.to_string_lossy().to_string());
    let project =
        crate::application::node::project_tiers::project_auto_fail_for_path(state, &primary_repo)
            .await
            .unwrap_or(None);

    // Instance tier: stored → env → false.
    let instance = state
        .settings
        .defaults()
        .await
        .map(|c| c.auto_fail)
        .unwrap_or(false);

    auto_fail::resolve_auto_fail(&auto_fail::AutoFailTiers {
        node,
        run: run_state.auto_fail,
        project,
        instance,
    })
}

/// `FailNode`: the node's Agent (`pdo fail`) declares its iteration failed. The
/// node fails; the Run fails too when `auto_fail` is opted in, and waits on its
/// user otherwise (ADR-0049).
#[derive(Debug, Clone)]
pub struct FailNode {
    pub run_id: String,
    pub node_id: String,
    pub iter: Option<i64>,
    pub reason: String,
}

impl Runs {
    pub async fn fail_node(self: &Arc<Self>, actor: &Actor, command: FailNode) -> Response {
        let FailNode {
            run_id,
            node_id,
            iter,
            reason,
        } = command;
        if let Err(denied) = self
            .authorize(actor, &on_node("fail", &run_id, &node_id))
            .await
        {
            return denied.into_response();
        }
        let state = self;

        let iter = iter.unwrap_or(1);

        // Resolve `auto_fail` BEFORE the terminal append, so the tail knows whether this
        // deliberate `pdo fail` terminalises the run or parks it `AwaitingUser`. The
        // `NodeFailed` itself is unconditional — the agent's iteration genuinely failed.
        let auto_fail_run = match reload_run_state(state, &run_id).await {
            Some((_, rs)) => resolve_run_auto_fail(state, &rs, &node_id).await,
            // No projected state: default to the safe park behaviour; the terminal
            // append below is a no-op anyway.
            None => false,
        };

        let event = event::Event {
            id: None,
            run_id: run_id.clone(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event::EventKind::NodeFailed,
            node_id: Some(node_id.clone()),
            iter: Some(iter),
            payload: Some(serde_json::json!({ "reason": reason })),
        };

        if let Err(e) = append_event(state, &event).await {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }

        // Same self-cancellation window as `node_done`: the reap kills `pdo fail`'s own
        // session, so inline the `RunFailed` append below could be silently dropped,
        // leaving the run `running` forever with a Failed node.
        let tail_state = state.clone();
        let tail_run = run_id.clone();
        let tail_node = node_id.clone();
        let tail_reason = reason.clone();
        detach_terminal_tail("node_fail", state.clone(), run_id, node_id, async move {
            // Snapshot the pane then kill the session; post-mortem inspection survives
            // via the snapshot. Capture repo_root AND the sandbox mode from the SAME
            // projection — the targeted container kill needs the mode.
            let (repo_root, tail_sandbox) = match load_events(&tail_state, &tail_run).await {
                Ok(evs) => projection::project(&evs)
                    .map(|s| (effective_repo_root(&tail_state, &s), !s.sandbox.is_off()))
                    .unwrap_or_else(|| (tail_state.repo_root.clone(), false)),
                Err(_) => (tail_state.repo_root.clone(), false),
            };
            reap_node_session(
                &tail_state,
                &repo_root,
                &tail_run,
                &tail_node,
                iter,
                tail_sandbox,
            );

            // An agent `pdo fail` terminalises the run to `Failed` ONLY when `auto_fail`
            // is opted in; otherwise it parks `AwaitingUser` for a human to confirm. Only
            // the agent `pdo fail` honours the opt-in — every runtime give-up parks.
            let run_event = if auto_fail_run {
                event::Event {
                    id: None,
                    run_id: tail_run.clone(),
                    ts: pdo_kernel::clock::now_iso(),
                    kind: event::EventKind::RunFailed,
                    node_id: None,
                    iter: None,
                    payload: Some(serde_json::json!({ "reason": tail_reason })),
                }
            } else {
                event::Event {
                    id: None,
                    run_id: tail_run.clone(),
                    ts: pdo_kernel::clock::now_iso(),
                    kind: event::EventKind::RunInterrupted,
                    node_id: None,
                    iter: None,
                    payload: Some(projection::interrupt_payload(
                        "agent_fail_awaiting",
                        format!(
                        "agent_fail_awaiting: agent pdo fail (awaiting confirmation): {tail_reason}"
                    ),
                    )),
                }
            };
            if let Err(e) = append_event(&tail_state, &run_event).await {
                error!("failed to append run terminal/park event: {e}");
            }

            // Failed or parked, the node's session was reaped and freed a slot.
            retry_waiting_nodes(&tail_state).await;

            info!("Node {tail_node} failed in run {tail_run} (auto_fail={auto_fail_run})");
        });
        (StatusCode::OK, "ok").into_response()
    }
}
