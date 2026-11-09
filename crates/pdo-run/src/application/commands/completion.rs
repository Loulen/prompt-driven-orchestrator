//! `mark_node_done`: the Pipeline Manager completes a node in its agent's place.

use super::*;

pub(super) async fn mark_node_done(
    state: Arc<Runs>,
    run_id: String,
    node_id: String,
    iter: i64,
) -> Response {
    // NOT `load_projected`, which would 404 on an unstarted run: this arm
    // needs the `Option<RunState>` itself, so `None` maps to `Allow` and
    // falls back on a synthetic empty `RunState`.
    let events = match load_events(&state, &run_id).await {
        Ok(e) => e,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("error: {e}") })),
            )
                .into_response();
        }
    };
    let run_state = projection::project(&events);

    // ADR-0049: completing a node on a terminal (or incident-parked) Run
    // embeds the re-open, so the completion lands on a live Run instead of
    // the guard's "resume the run first" 409.
    let run_state = match run_state {
        Some(rs) => {
            match crate::application::reopen::embed_reopen_for_targeted_command(&state, &run_id, rs)
                .await
            {
                Ok(s) => Some(s),
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": format!("error: {e}") })),
                    )
                        .into_response();
                }
            }
        }
        None => None,
    };

    // Validate against the projected state BEFORE any side effect (output
    // validation, append, downstream dispatch) — the same pure decision as
    // `node_done` and `node_skip`. The typed stop keeps a legal duplicate a
    // `200` while a reject becomes `409 {"error":"completion_rejected",…}`.
    // The invariant cannot live on `CompletionAttempt`: this arm never
    // builds one, and it is the whole UI path.
    if let Some(stop) = completion_head_gate(
        run_advance::evaluate_completion_head(run_state.as_ref(), &run_id, &node_id, iter),
        "mark_node_done",
        &run_id,
        &node_id,
        iter,
    ) {
        return stop.into_response();
    }

    let empty_run_state = state::RunState::new(run_id.clone(), String::new());
    let rs_ref = run_state.as_ref().unwrap_or(&empty_run_state);
    let repo_root = effective_repo_root(&state, rs_ref);
    let pipeline_name = run_state
        .as_ref()
        .map(|rs| rs.pipeline_name.as_str())
        .unwrap_or("");
    let pipeline_path = resolve_run_pipeline_path(&repo_root, &run_id, pipeline_name);
    let worktree_dir = worktree_dir_for_run(&repo_root, &run_id);
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");

    // The shared chokepoint (#490). Both surfaces project the refusal
    // through the same single function, which is what makes "a refusal is
    // never a 2xx" cover `POST /commands` too.
    if let Some(refusal) = check_output_validation_with_retry(
        &state,
        &pipeline_path,
        &node_id,
        iter,
        &artifacts_dir,
        &run_id,
        rs_ref,
    )
    .await
    {
        return crate::adapters::http::verdicts::refusal_response(&refusal);
    }

    // #654 / ADR-0060: the manual completion delivers like every other
    // one. Pre-#654 this arm ran neither the merge-back nor any
    // worktree check, so an interactive node marked complete from the UI
    // left its sub-worktree stranded and its shared-worktree edits
    // uncommitted — the asymmetry ADR-0035 recorded as an accepted limit
    // and this ticket removes. Same single operation, same events, before
    // the terminal append and therefore before the downstream spawn — and
    // after the output validation, so a mark-done refused on its outputs
    // leaves no commit and no `NodeDelivered` behind.
    if let Some(refusal) = crate::application::complete_node::deliver_node_run(
        &state,
        &events,
        rs_ref,
        &repo_root,
        &worktree_dir,
        &run_id,
        &node_id,
        iter,
    )
    .await
    {
        return crate::adapters::http::verdicts::refusal_response(&refusal);
    }

    let event = event::Event {
        id: None,
        run_id: run_id.clone(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::NodeCompleted,
        node_id: Some(node_id.clone()),
        iter: Some(iter),
        payload: Some(serde_json::json!({ "source": "mark_node_done" })),
    };

    if let Err(e) = append_event(&state, &event).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("error: {e}") })),
        )
            .into_response();
    }

    let cmd_event = event::Event {
        id: None,
        run_id: run_id.clone(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::CommandIssued,
        node_id: Some(node_id.clone()),
        iter: Some(iter),
        payload: Some(serde_json::json!({
            "command": "mark_node_done",
            "node_id": node_id,
            "iter": iter,
        })),
    };
    if let Err(e) = append_event(&state, &cmd_event).await {
        error!("failed to append mark_node_done command event: {e}");
    }

    // A terminal transition reaps the node's session (snapshot, then kill),
    // like every other completion: a node marked done never keeps a live pane.
    // Before, only the agent's own completion reaped, so a node whose
    // `pdo complete` could not reach the daemon — a sandboxed one — and that a
    // human approved kept its session until the reaper's one-hour backstop.
    // Inline, not detached: this command never runs inside the session it kills.
    reap_node_session(
        &state,
        &repo_root,
        &run_id,
        &node_id,
        iter,
        !rs_ref.sandbox.is_off(),
    );

    // Shared post-`NodeCompleted` tail, `SweepFirst`: advance the run +
    // re-drive throttled waiters, THEN fire this node's edges, then the
    // single completion gate. flag = true because the just-finished node was
    // interactive, so the run can still project `AwaitingUser` at the gate
    // and must still complete — unlike the other sites (flag = false).
    run_advance::complete_node(
        &state,
        &run_id,
        &node_id,
        run_advance::CompletionOrder::SweepFirst,
        true,
    )
    .await;

    info!("mark_node_done: node {node_id} in run {run_id}");
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}
