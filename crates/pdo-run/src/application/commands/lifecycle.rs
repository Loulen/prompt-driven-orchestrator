//! The commands on the Run itself: pause, resume, reopen, rename, and the
//! artifact the Manager injects into its blackboard.

use super::*;

pub(super) async fn pause_run(state: Arc<Runs>, run_id: String) -> Response {
    let (_, run_state) = match load_projected(&state, &run_id).await {
        Ok(v) => v,
        Err(resp) => return *resp,
    };

    if !matches!(
        run_state.status,
        state::RunStatus::Running | state::RunStatus::AwaitingUser
    ) {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": format!("cannot pause run in {:?} state", run_state.status)
            })),
        )
            .into_response();
    }

    let pause_event = event::Event {
        id: None,
        run_id: run_id.clone(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::RunPaused,
        node_id: None,
        iter: None,
        payload: None,
    };
    if let Err(e) = append_event(&state, &pause_event).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("error: {e}") })),
        )
            .into_response();
    }

    info!("pause_run: run {run_id}");
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

pub(super) async fn resume_run(state: Arc<Runs>, run_id: String) -> Response {
    let (_, run_state) = match load_projected(&state, &run_id).await {
        Ok(v) => v,
        Err(resp) => return *resp,
    };

    match run_state.status {
        state::RunStatus::Paused => {
            let resume_event = event::Event {
                id: None,
                run_id: run_id.clone(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event::EventKind::RunResumed,
                node_id: None,
                iter: None,
                payload: None,
            };
            if let Err(e) = append_event(&state, &resume_event).await {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("error: {e}") })),
                )
                    .into_response();
            }
        }
        state::RunStatus::Halted | state::RunStatus::Failed => {
            let cmd_event = event::Event {
                id: None,
                run_id: run_id.clone(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event::EventKind::CommandIssued,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "command": "resume_run" })),
            };
            if let Err(e) = append_event(&state, &cmd_event).await {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("error: {e}") })),
                )
                    .into_response();
            }
        }
        _ => {}
    }

    // A resumable run re-drives the git merge in its pipeline worktree; kill
    // any open shell so its uncommitted edits can't race it. Refusing with a
    // 409 would deadlock: a shell only dies on archive, itself reachable only
    // from a terminal state.
    state
        .tmux()
        .kill(&pdo_session::domain::names::shell_session_name(&run_id));

    // Re-arm a sandboxed Run's container before the scheduler `docker exec`s
    // into it. Containers are created without `--restart` and `boot_recovery`
    // skips terminal Runs, so after a host reboot the container is down and
    // reviving the Run would spawn into a dead one. Resurrect it (via
    // `spawn_blocking`, `ensure_ready` may `docker build`) or fail
    // EXPLICITLY — never a silent host fallback.
    if !run_state.sandbox.is_off() {
        let prep = match run_sandbox::context_from_state(&state, &run_state).await {
            Ok(ctx) => tokio::task::spawn_blocking(move || ctx.ensure_ready())
                .await
                .unwrap_or_else(|je| Err(anyhow::anyhow!("sandbox ensure_ready panicked: {je}"))),
            Err(e) => Err(e),
        };
        if let Err(e) = prep {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("sandbox container unavailable: {e:#}")
                })),
            )
                .into_response();
        }
        // Record that the container is up again, or the spawn precondition
        // refuses every node the re-evaluation below proposes. Load-bearing
        // for a Run that failed *during* its own prep: its projection is
        // still `pending` and resuming is the operator's only recovery path.
        // Emitted only after `ensure_ready` returned `Ok`, and only when the
        // Run is actually blocked, so a routine resume adds no no-op event.
        if run_state.sandbox_spawn_block().is_some() {
            mark_sandbox_prep_ready(&state, &run_id).await;
        }
    }

    let summary = re_evaluate_after_command(&state, &run_id).await;
    drive_after_reopen(&state, &run_id, &summary).await;

    info!("resume_run: run {run_id}");
    (StatusCode::OK, Json(summary.into_response_body())).into_response()
}

/// The readiness tick after a resume or a reopen re-evaluated the edges of
/// the completed nodes: it spawns what no edge reaches — a dropped
/// `interrupted`/`stopped` node fed by `Start` only — and runs what a
/// completion during a pause skipped (the auto-skip sweep, pending loop seeds,
/// the completion gate). A no-op once the re-evaluation ended the Run.
async fn drive_after_reopen(state: &Runs, run_id: &str, summary: &ReEvalSummary) {
    if summary.terminal.is_none() {
        // Boxed: the advance tick is a large future, and inlined into every
        // command handler it overflows the stack of a debug build.
        Box::pin(
            crate::application::scheduler::node_completion::spawn_ready_after_event(state, run_id),
        )
        .await;
    }
}

pub(super) async fn reopen_run(state: Arc<Runs>, run_id: String) -> Response {
    let (_, run_state) = match load_projected(&state, &run_id).await {
        Ok(v) => v,
        Err(resp) => return *resp,
    };

    // Re-openable = any terminal Run, or an incident-parked `AwaitingUser`.
    // An interactive `AwaitingUser` and a cleanly `Running`/`Paused` Run are
    // refused loudly: reopen never overrides a node's user wait, and a live
    // Run needs no re-projection.
    let reopenable = run_state.status.is_terminal()
        && run_state.status != state::RunStatus::Archived
        || (run_state.status == state::RunStatus::AwaitingUser
            && run_state.awaiting_reason.is_some());
    if !reopenable {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "not_reopenable",
                "message": format!(
                    "run {run_id} is {:?}: reopen_run only re-opens a terminal \
                     or incident-parked run",
                    run_state.status
                ),
            })),
        )
            .into_response();
    }

    // The projection lifts the Run to `Running`, freezes satisfied
    // `(node, iter)` and drops interrupted nodes so they re-drive. The
    // terminal label stays in the log.
    let cmd_event = event::Event {
        id: None,
        run_id: run_id.clone(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::CommandIssued,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "command": "reopen_run" })),
    };
    if let Err(e) = append_event(&state, &cmd_event).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("error: {e}") })),
        )
            .into_response();
    }

    // Kill any open shell before the re-drive re-arms the merge.
    state
        .tmux()
        .kill(&pdo_session::domain::names::shell_session_name(&run_id));

    // Re-arm a sandboxed Run's container before the scheduler `docker exec`s
    // into it (same guard as `resume_run`).
    if !run_state.sandbox.is_off() {
        let prep = match run_sandbox::context_from_state(&state, &run_state).await {
            Ok(ctx) => tokio::task::spawn_blocking(move || ctx.ensure_ready())
                .await
                .unwrap_or_else(|je| Err(anyhow::anyhow!("sandbox ensure_ready panicked: {je}"))),
            Err(e) => Err(e),
        };
        if let Err(e) = prep {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("sandbox container unavailable: {e:#}")
                })),
            )
                .into_response();
        }
        if run_state.sandbox_spawn_block().is_some() {
            mark_sandbox_prep_ready(&state, &run_id).await;
        }
    }

    let summary = re_evaluate_after_command(&state, &run_id).await;
    drive_after_reopen(&state, &run_id, &summary).await;

    info!("reopen_run: run {run_id}");
    (StatusCode::OK, Json(summary.into_response_body())).into_response()
}

pub(super) async fn inject_artifact(
    state: Arc<Runs>,
    run_id: String,
    path: String,
    content: String,
) -> Response {
    // `path` is already proven relative and `..`-free by the parse.
    let requested = std::path::Path::new(&path);

    // NOT `load_projected`, which would turn this arm's 200 into a 404: it is
    // designed to write the artifact a Run has not produced yet, so a missing
    // projection and a DB error both degrade to the daemon's own repo root and
    // the write proceeds.
    let repo_root = match load_events(&state, &run_id).await {
        Ok(events) => match projection::project(&events) {
            Some(run_state) => effective_repo_root(&state, &run_state),
            None => state.repo_root.clone(),
        },
        Err(_) => state.repo_root.clone(),
    };
    let worktree_dir = worktree_dir_for_run(&repo_root, &run_id);
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
    let full_path = artifacts_dir.join(requested);

    if let Some(parent) = full_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("failed to create dir: {e}") })),
            )
                .into_response();
        }
    }
    if let Err(e) = std::fs::write(&full_path, &content) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("failed to write artifact: {e}") })),
        )
            .into_response();
    }

    let cmd_event = event::Event {
        id: None,
        run_id: run_id.clone(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::CommandIssued,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({
            "command": "inject_artifact",
            "path": path,
        })),
    };
    if let Err(e) = append_event(&state, &cmd_event).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("error: {e}") })),
        )
            .into_response();
    }

    // Injecting the artifact a parked node was waiting on embeds the re-open,
    // so the freshly-provided output unblocks the downstream in one
    // round-trip. A live Run is left to its own tick.
    if let Some((_, run_state)) = reload_run_state(&state, &run_id).await {
        let needs_reopen = (run_state.status.is_terminal()
            && run_state.status != state::RunStatus::Archived)
            || (run_state.status == state::RunStatus::AwaitingUser
                && run_state.awaiting_reason.is_some());
        if needs_reopen {
            if let Err(e) = crate::application::reopen::embed_reopen_for_targeted_command(
                &state, &run_id, run_state,
            )
            .await
            {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("error: {e}") })),
                )
                    .into_response();
            }
            re_evaluate_after_command(&state, &run_id).await;
        }
    }

    info!("inject_artifact: {path} in run {run_id}");
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

pub(super) async fn rename_run(state: Arc<Runs>, run_id: String, new_name: String) -> Response {
    let rename_event = event::Event {
        id: None,
        run_id: run_id.clone(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::RunRenamed,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "name": new_name })),
    };
    if let Err(e) = append_event(&state, &rename_event).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("error: {e}") })),
        )
            .into_response();
    }

    info!("rename_run: run {run_id} renamed to {:?}", new_name);
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}
