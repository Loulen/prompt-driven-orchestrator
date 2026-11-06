//! `StopNode`: the canvas's Stop button.

use std::sync::Arc;

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tracing::{error, info};

use crate::application::events::append_event;
use crate::application::events::load_events;
use crate::application::node::primitives as node_primitives;
use crate::application::node::reap::reap_node_session;
use crate::application::operations::on_node;
use crate::application::paths::effective_repo_root;
use crate::application::scheduler::node_completion::retry_waiting_nodes;
use crate::domain::{projection, state};
use crate::Runs;
use pdo_kernel::Actor;

/// `StopNode`: the operator stops a running node (the canvas's Stop button). The
/// node is `stopped`, never failed (ADR-0009).
#[derive(Debug, Clone)]
pub struct StopNode {
    pub run_id: String,
    pub node_id: String,
}

impl Runs {
    pub async fn stop_node(self: &Arc<Self>, actor: &Actor, command: StopNode) -> Response {
        let StopNode { run_id, node_id } = command;
        if let Err(denied) = self
            .authorize(actor, &on_node("stop", &run_id, &node_id))
            .await
        {
            return denied.into_response();
        }
        let state = self;

        let events = match load_events(state, &run_id).await {
            Ok(e) => e,
            Err(e) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
            }
        };
        let run_state = match projection::project(&events) {
            Some(s) => s,
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({ "error": "run not found" })),
                )
                    .into_response();
            }
        };

        let ns = match run_state.nodes.get(&node_id) {
            Some(ns) if ns.status == state::NodeStatus::Running => ns,
            Some(_) => {
                return (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({ "error": "node is not running" })),
                )
                    .into_response();
            }
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({ "error": format!("node '{node_id}' not found in run state") })),
                )
                    .into_response();
            }
        };

        let iter = ns.iter;

        // Snapshot the pane BEFORE the session is killed, so the stopped node's
        // post-mortem pane survives. `stop_node` kills the session too, idempotently.
        let repo_root = effective_repo_root(state, &run_state);
        reap_node_session(
            state,
            &repo_root,
            &run_id,
            &node_id,
            iter,
            !run_state.sandbox.is_off(),
        );

        let params = node_primitives::StopNodeParams {
            run_id: &run_id,
            node_id: &node_id,
            iter,
            tmux: state.tmux(),
        };

        let result = node_primitives::stop_node(&params);

        for ev in &result.events {
            if let Err(e) = append_event(state, ev).await {
                error!("failed to append event: {e}");
            }
        }

        // Stopping a running node freed its session slot.
        retry_waiting_nodes(state).await;
        // ADR-0086: it may have been the last started node a Run past `End`
        // waited for.
        crate::application::scheduler::node_completion::reevaluate_end(state, &run_id).await;

        info!("node_stop: stopped {node_id} iter {iter} in run {run_id}");
        (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
    }
}
