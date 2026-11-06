//! The commands on a loop: extend a legacy `loop` node's cycle, bump or end a
//! region, set a bounded region's cap in flight.

use super::*;

pub(super) async fn extend_cycle(
    state: Arc<Runs>,
    run_id: String,
    node_id: String,
    additional_iter: i64,
) -> Response {
    // Validate against the run's pipeline SNAPSHOT before any event is
    // appended: a rejected command must leave no trace in the log, and a
    // library edit after launch must not affect an in-flight run.
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
        if !pipeline.nodes.iter().any(|n| n.id == node_id) {
            return crate::application::node_known::node_not_found(&node_id);
        }
        if let Some(region) = loop_region::bounded_region_for_member(&pipeline, &node_id) {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": format!(
                        "node '{node_id}' is a member of loop region '{}'; \
                         use bump_region with region_id '{}'",
                        region.id, region.id
                    )
                })),
            )
                .into_response();
        }
    } else {
        // An unreadable snapshot can't be validated against; stay permissive
        // rather than reject blind.
        warn!("extend_cycle: pipeline snapshot unreadable for run {run_id}; skipping target validation");
    }

    let cmd_event = event::Event {
        id: None,
        run_id: run_id.clone(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::CommandIssued,
        node_id: Some(node_id.clone()),
        iter: None,
        payload: Some(serde_json::json!({
            "command": "extend_cycle",
            "node_id": node_id,
            "additional_iter": additional_iter,
        })),
    };
    if let Err(e) = append_event(&state, &cmd_event).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("error: {e}") })),
        )
            .into_response();
    }

    if run_state.status == state::RunStatus::Halted || run_state.status == state::RunStatus::Failed
    {
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
            error!("failed to append resume_run: {e}");
        }
    }

    let summary = re_evaluate_after_command(&state, &run_id).await;

    info!("extend_cycle: node {node_id} +{additional_iter} in run {run_id}");
    (StatusCode::OK, Json(summary.into_response_body())).into_response()
}

pub(super) async fn region(
    state: Arc<Runs>,
    run_id: String,
    region_id: String,
    action: RegionAction,
    kind_str: &'static str,
) -> Response {
    let payload = match &action {
        RegionAction::Bump { additional_iter } => serde_json::json!({
            "command": "bump_region",
            "region_id": region_id,
            "additional_iter": additional_iter,
        }),
        RegionAction::End => serde_json::json!({
            "command": "end_region",
            "region_id": region_id,
        }),
    };

    // Validate against the run's pipeline SNAPSHOT before any event is
    // appended: an unknown region_id must leave no trace in the log.
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
        if !pipeline.loops.iter().any(|r| r.id == region_id) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("region '{region_id}' not found in pipeline")
                })),
            )
                .into_response();
        }
    } else {
        warn!(
            "{kind_str}: pipeline snapshot unreadable for run {run_id}; skipping region validation"
        );
    }

    let cmd_event = event::Event {
        id: None,
        run_id: run_id.clone(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::CommandIssued,
        node_id: None,
        iter: None,
        payload: Some(payload),
    };
    if let Err(e) = append_event(&state, &cmd_event).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("error: {e}") })),
        )
            .into_response();
    }

    // An exhausted-unrouted region parks the run, so lift it back to
    // `Running` before re-evaluating. An interactive `AwaitingUser` (no
    // incident reason) is left alone: routing a region never overrides a
    // node's genuine user wait.
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
            error!("failed to append resume_run after region route: {e}");
        }
    }

    let summary = re_evaluate_after_command(&state, &run_id).await;

    info!("{kind_str}: region {region_id} in run {run_id}");
    (StatusCode::OK, Json(summary.into_response_body())).into_response()
}

pub(super) async fn set_region_max_iter(
    state: Arc<Runs>,
    run_id: String,
    region_id: String,
    max_iter: i64,
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
        if !pipeline.loops.iter().any(|r| r.id == region_id) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("region '{region_id}' not found in pipeline")
                })),
            )
                .into_response();
        }
    } else {
        warn!("set_region_max_iter: pipeline snapshot unreadable for run {run_id}; skipping region validation");
    }

    let cmd_event = event::Event {
        id: None,
        run_id: run_id.clone(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::CommandIssued,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({
            "command": "set_region_max_iter",
            "region_id": region_id,
            "max_iter": max_iter,
        })),
    };
    if let Err(e) = append_event(&state, &cmd_event).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
    }

    // Lift a parked run so the raised cap re-drives the region now. An
    // interactive `AwaitingUser` is left alone.
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
            error!("failed to append resume_run after set_region_max_iter: {e}");
        }
    }

    let summary = re_evaluate_after_command(&state, &run_id).await;
    info!("set_region_max_iter: region {region_id} -> {max_iter} in run {run_id}");
    (StatusCode::OK, Json(summary.into_response_body())).into_response()
}
