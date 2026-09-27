//! `skip_node`: a node skipped **locally** — satisfied with an empty (or
//! overridden) output, the Run going on.

use super::*;

pub(super) async fn skip_node(
    state: Arc<Runs>,
    run_id: String,
    node_id: String,
    iter: i64,
    overrides: Option<HashMap<String, String>>,
) -> Response {
    let events = match load_events(&state, &run_id).await {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };
    let run_state = projection::project(&events);

    // Embed the re-open: skipping on a terminal / incident-parked run lifts
    // it first, so the completion gate does not refuse with "resume first".
    let run_state = match run_state {
        Some(rs) => {
            match crate::application::reopen::embed_reopen_for_targeted_command(&state, &run_id, rs)
                .await
            {
                Ok(s) => Some(s),
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                        .into_response();
                }
            }
        }
        None => None,
    };

    // Same head-gate as `mark_node_done`: a duplicate skip is rejected or
    // no-op'd, never double-appended.
    if let Some(stop) = completion_head_gate(
        run_advance::evaluate_skip_completion_head(run_state.as_ref(), &run_id, &node_id, iter),
        "skip_node",
        &run_id,
        &node_id,
        iter,
    ) {
        return stop.into_response();
    }

    let empty_run_state = state::RunState::new(run_id.clone(), String::new());
    let rs_ref = run_state.as_ref().unwrap_or(&empty_run_state);
    let repo_root = effective_repo_root(&state, rs_ref);
    let pipeline_path = resolve_run_pipeline_path(&repo_root, &run_id, &rs_ref.pipeline_name);
    let worktree_dir = worktree_dir_for_run(&repo_root, &run_id);
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");

    // Write the skipped node's outputs, so the downstream resolver finds a
    // real (if empty) artifact instead of a missing file that would read as
    // "not produced". An unreadable snapshot hides the node's ports; deposit
    // a single default `output`.
    let overrides = overrides.unwrap_or_default();
    let write_result = match std::fs::read_to_string(&pipeline_path)
        .ok()
        .and_then(|yaml| pipeline::parse_pipeline(&yaml).ok())
        .map(|p| p.pipeline)
    {
        Some(p) => crate::application::node::primitives::write_skip_outputs(
            &p,
            &node_id,
            iter,
            &overrides,
            &artifacts_dir,
        ),
        None => {
            let one: HashMap<String, String> = [(
                "output".to_string(),
                overrides.get("output").cloned().unwrap_or_default(),
            )]
            .into_iter()
            .collect();
            crate::application::node::primitives::inject_outputs(
                &crate::application::node::primitives::InjectOutputsParams {
                    node_id: &node_id,
                    iter,
                    artifacts: &one,
                    artifacts_dir: &artifacts_dir,
                },
            );
            Ok(vec!["output".to_string()])
        }
    };
    if let Err(reason) = write_result {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": reason })),
        )
            .into_response();
    }

    // Deliberately NO output validation and NO sub-worktree merge (the whole
    // point is an empty/dummy output), and — unlike `node_skip` — NO
    // `RunSkipped`: the run stays live.
    let node_completed = event::Event {
        id: None,
        run_id: run_id.clone(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::NodeCompleted,
        node_id: Some(node_id.clone()),
        iter: Some(iter),
        payload: Some(serde_json::json!({
            "source": "skip_node",
            "skipped": true,
            "reason": "skipped locally by operator",
            "override_ports": overrides.keys().cloned().collect::<Vec<_>>(),
        })),
    };
    if let Err(e) = append_event(&state, &node_completed).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
    }

    let cmd_event = event::Event {
        id: None,
        run_id: run_id.clone(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::CommandIssued,
        node_id: Some(node_id.clone()),
        iter: Some(iter),
        payload: Some(serde_json::json!({
            "command": "skip_node",
            "node_id": node_id,
            "iter": iter,
        })),
    };
    if let Err(e) = append_event(&state, &cmd_event).await {
        error!("failed to append skip_node command event: {e}");
    }

    // Same post-completion tail as `mark_node_done`, flag=false (a skipped
    // node is never interactive).
    run_advance::complete_node(
        &state,
        &run_id,
        &node_id,
        run_advance::CompletionOrder::SweepFirst,
        false,
    )
    .await;

    info!("skip_node: node {node_id} iter {iter} skipped locally in run {run_id}");
    (
        StatusCode::OK,
        Json(serde_json::json!({ "ok": true, "skipped": true })),
    )
        .into_response()
}
