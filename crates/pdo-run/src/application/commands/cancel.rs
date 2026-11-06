//! `cancel_run`: the operator gives up on a live Run. It stops launching
//! nodes, every node holding a session is stopped (never failed, ADR-0009),
//! and the Run ends `halted` with the reason « cancelled »: terminal, so it can
//! be retried whole (`retry_all`) or reopened (`reopen_run`).

use super::*;
use crate::application::node::primitives as node_primitives;

/// The reason a cancelled Run carries, on its banner and in `pdo run status`.
pub(super) const CANCELLED: &str = "cancelled";

pub(super) async fn cancel_run(state: Arc<Runs>, run_id: String) -> Response {
    let (_, run_state) = match load_projected(&state, &run_id).await {
        Ok(v) => v,
        Err(resp) => return *resp,
    };
    if !run_state.status.is_live() {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": format!("cannot cancel run in {:?} state", run_state.status)
            })),
        )
            .into_response();
    }

    // Paused first: the scheduler launches nothing more while the sessions go.
    if matches!(
        run_state.status,
        state::RunStatus::Running | state::RunStatus::AwaitingUser
    ) {
        if let Err(resp) = append(
            &state,
            run_event(&run_id, event::EventKind::RunPaused, None),
        )
        .await
        {
            return *resp;
        }
    }

    let repo_root = effective_repo_root(&state, &run_state);
    let sandboxed = !run_state.sandbox.is_off();
    let mut stopped = Vec::new();
    for (node_id, node) in &run_state.nodes {
        if !node.status.holds_session() {
            continue;
        }
        // The pane is captured before the session dies, as the Stop button does.
        reap_node_session(&state, &repo_root, &run_id, node_id, node.iter, sandboxed);
        let result = node_primitives::stop_node(&node_primitives::StopNodeParams {
            run_id: &run_id,
            node_id,
            iter: node.iter,
            tmux: state.tmux(),
        });
        // A stop the log does not hold would leave the node `running` in a
        // halted Run: answer the error, the Run stays paused, and a second
        // cancel stops what is left.
        for ev in &result.events {
            if let Err(e) = append_event(&state, ev).await {
                error!("cancel_run: cannot record the stop of {node_id}: {e}");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": format!("cannot record the stop of {node_id}: {e}")
                    })),
                )
                    .into_response();
            }
        }
        stopped.push(node_id.clone());
    }
    stopped.sort();

    let halted = run_event(
        &run_id,
        event::EventKind::RunHalted,
        Some(serde_json::json!({ "message": CANCELLED, "command": "cancel_run" })),
    );
    if let Err(resp) = append(&state, halted).await {
        return *resp;
    }
    // The stopped sessions freed their admission slots.
    retry_waiting_nodes(&state).await;

    info!(
        "cancel_run: run {run_id} cancelled, {} node(s) stopped",
        stopped.len()
    );
    (
        StatusCode::OK,
        Json(serde_json::json!({ "status": "halted", "reason": CANCELLED, "stopped": stopped })),
    )
        .into_response()
}

fn run_event(
    run_id: &str,
    kind: event::EventKind,
    payload: Option<serde_json::Value>,
) -> event::Event {
    event::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: pdo_kernel::clock::now_iso(),
        kind,
        node_id: None,
        iter: None,
        payload,
    }
}

async fn append(state: &Runs, ev: event::Event) -> Result<(), Box<Response>> {
    append_event(state, &ev).await.map_err(|e| {
        Box::new(
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("error: {e}") })),
            )
                .into_response(),
        )
    })
}
