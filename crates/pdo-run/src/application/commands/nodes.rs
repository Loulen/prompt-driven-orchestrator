//! `kill_node` and `start_node`: the Manager kills a node's work in progress, or
//! force-spawns a node out of dependency order.

use super::*;

pub(super) async fn kill_node(
    state: Arc<Runs>,
    run_id: String,
    node_id: String,
    iter: i64,
) -> Response {
    // READ-ONLY, and BEFORE any append: one projection yields both the
    // `repo_root` the snapshot goes under (a Run may target a repo other than
    // the daemon's) and the sandbox flag the in-container kill needs. Both
    // must come from the SAME projection. `reload_run_state`, not
    // `load_projected`: both of its failure modes mean the same thing here —
    // no overridden repo, no container to kill — so neither may become a 4xx.
    let projected = reload_run_state(&state, &run_id).await.map(|(_, s)| s);
    let (repo_root, kill_sandbox) = projected
        .as_ref()
        .map(|s| (effective_repo_root(&state, s), !s.sandbox.is_off()))
        .unwrap_or_else(|| (state.repo_root.clone(), false));

    // THE TERMINAL EVENT FIRST, THE REAP SECOND. Reaping first opens a
    // "dead session / projection still Running" window that `GET …/pane`
    // answers by relaunching the harness; and on an append error the node
    // would stay `Running` forever with its session already killed and no
    // audit event. Appending first makes a 500 mean "nothing happened,
    // retry". Accepted risk: a few ms where the node is `Failed` with a live
    // session, which `GET …/pane` honestly reports as `live`.
    let fail_event = event::Event {
        id: None,
        run_id: run_id.clone(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::NodeFailed,
        node_id: Some(node_id.clone()),
        iter: Some(iter),
        payload: Some(serde_json::json!({
            "reason": "killed via kill_node command",
            "source": "kill_node",
        })),
    };
    // The guard ignores a failure on an iteration already terminal: the kill
    // then kills nothing, and the answer says so instead of `ok` alone.
    let ignored = match transition_guard::validate_transition(projected.as_ref(), &fail_event) {
        transition_guard::Verdict::NoOp { reason } => Some(reason),
        _ => None,
    };
    if let Err(e) = append_event(&state, &fail_event).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("error: {e}") })),
        )
            .into_response();
    }

    // The helper already contains the tmux kill AND the best-effort session
    // kill, preceded by the pane capture; adding either alongside would double
    // the `docker exec`. Called unconditionally, including when the append
    // above was no-op'd by the transition guard, so a second `kill_node` stays
    // an idempotent cleanup — and the first snapshot stays immutable, since a
    // dead session makes `capture` return `None`.
    reap_node_session(&state, &repo_root, &run_id, &node_id, iter, kill_sandbox);

    let cmd_event = event::Event {
        id: None,
        run_id: run_id.clone(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::CommandIssued,
        node_id: Some(node_id.clone()),
        iter: Some(iter),
        payload: Some(serde_json::json!({
            "command": "kill_node",
            "node_id": node_id,
            "iter": iter,
        })),
    };
    if let Err(e) = append_event(&state, &cmd_event).await {
        error!("failed to append kill_node command event: {e}");
    }

    // Killing a node FREES an admission slot, so re-drive the nodes throttled
    // into `waiting`. `retry_waiting_nodes` has no timer of its own — every
    // caller is event-driven — so a `restart_node` that answered
    // `waiting:true` would otherwise starve forever.
    retry_waiting_nodes(&state).await;

    if let Some(reason) = ignored {
        info!("kill_node no-op for {node_id} iter {iter} in run {run_id}: {reason}");
        return (
            StatusCode::OK,
            Json(serde_json::json!({ "ok": true, "noop": true, "reason": reason })),
        )
            .into_response();
    }
    info!("kill_node: node {node_id} iter {iter} in run {run_id}");
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

pub(super) async fn start_node(
    state: Arc<Runs>,
    run_id: String,
    node_id: String,
    overrides: Option<HashMap<String, String>>,
) -> Response {
    // Force-spawn a node out of dependency order. The manager twin of the UI
    // Start button: both funnel through `force_spawn_node`, which derives the
    // iteration and owns the run-status and admission-cap guards. The wire
    // `iter` is dropped at parse time — a manager-pinned iter would fight that
    // derivation.

    // Audit the manager's intent before acting. Only the override port names
    // are logged, not their content, which lands in artifacts.
    let cmd_event = event::Event {
        id: None,
        run_id: run_id.clone(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::CommandIssued,
        node_id: Some(node_id.clone()),
        iter: None,
        payload: Some(serde_json::json!({
            "command": "start_node",
            "node_id": node_id,
            "override_ports": overrides
                .as_ref()
                .map(|o| o.keys().cloned().collect::<Vec<_>>()),
        })),
    };
    if let Err(e) = append_event(&state, &cmd_event).await {
        error!("failed to append start_node command event: {e}");
    }

    force_spawn_node(&state, &run_id, &node_id, overrides.as_ref()).await
}
