//! `force_route`: an explicit exit from a node or region, short-circuiting its
//! `when:` edges — the lever for a Run wedged `unrouted`.

use super::*;

pub(super) async fn force_route(
    state: Arc<Runs>,
    run_id: String,
    from: String,
    target: String,
) -> Response {
    let (_, run_state) = match load_projected(&state, &run_id).await {
        Ok(v) => v,
        Err(resp) => return *resp,
    };
    let repo_root = effective_repo_root(&state, &run_state);
    let pipeline_path = resolve_run_pipeline_path(&repo_root, &run_id, &run_state.pipeline_name);
    if let Some(pipeline) = std::fs::read_to_string(&pipeline_path)
        .ok()
        .and_then(|yaml| pipeline::parse_pipeline(&yaml).ok())
        .map(|p| p.pipeline)
    {
        let from_ok = pipeline.nodes.iter().any(|n| n.id == from)
            || pipeline.loops.iter().any(|r| r.id == from);
        if !from_ok {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("force_route source '{from}' is neither a node nor a region in the pipeline")
                })),
            )
                .into_response();
        }
        if !pipeline.nodes.iter().any(|n| n.id == target) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("force_route target '{target}' is not a node in the pipeline")
                })),
            )
                .into_response();
        }
    } else {
        warn!("force_route: pipeline snapshot unreadable for run {run_id}; skipping endpoint validation");
    }

    let cmd_event = event::Event {
        id: None,
        run_id: run_id.clone(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::CommandIssued,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({
            "command": "force_route",
            "from": from,
            "target": target,
        })),
    };
    if let Err(e) = append_event(&state, &cmd_event).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
    }

    let needs_reopen = matches!(
        run_state.status,
        state::RunStatus::Halted | state::RunStatus::Failed
    ) || (run_state.status == state::RunStatus::AwaitingUser
        && run_state.awaiting_reason.is_some());
    if needs_reopen {
        let resume_event = event::Event {
            id: None,
            run_id: run_id.clone(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event::EventKind::CommandIssued,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "command": "resume_run" })),
        };
        if let Err(e) = append_event(&state, &resume_event).await {
            error!("failed to append resume_run after force_route: {e}");
        }
    }

    let summary = re_evaluate_after_command(&state, &run_id).await;
    info!("force_route: {from} -> {target} in run {run_id}");
    (StatusCode::OK, Json(summary.into_response_body())).into_response()
}
