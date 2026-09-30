//! `restart_node`: the Manager restarts a node's iteration in place.

use super::*;

pub(super) async fn restart_node(
    state: Arc<Runs>,
    run_id: String,
    node_id: String,
    iter: i64,
) -> Response {
    // #489 / ADR-0037 — EVERY KNOWABLE CAUSE IS TESTED BEFORE THE KILL, AND
    // THE `SpawnOutcome` IS READ.
    //
    // Pre-#489 this arm killed the tmux session, appended its
    // `CommandIssued`, THEN discovered the Run / the pipeline / the node, and
    // finally dropped `spawn_node`'s return without so much as a `let _ =`.
    // Every one of the five `SpawnOutcome`s answered `200 {"ok":true}` — and
    // on an isolated node the spawn failed 100% of the time
    // (`git worktree add -b` on a branch that already exists, exit 255),
    // which is the whole of #489: session dead, zero events, node still
    // projected `Running`, and 30 s later the liveness sweep inventing
    // `session_died` — a false cause that sent operators after tmux for a git
    // bug.
    //
    // ADR-0025 §2's "validate before writing" now extends to the KILL, not
    // just the append. The order below is the contract.

    // PRE-KILL PROBE 1: the transition guard. NOT `load_projected`, same
    // reason as `mark_node_done`:
    // `validate_transition` takes an `Option<RunState>` and maps
    // `None -> Allow` deliberately, so an unstarted run must reach the guard
    // rather than be 404'd here.
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
    let projected = projection::project(&events);
    // A restart on a terminal (or incident-parked) Run re-opens it atomically
    // before the guard below sees it — no "resume then restart" race.
    let projected = match projected {
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
    let restart_probe = event::Event {
        id: None,
        run_id: run_id.clone(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::NodeStarted,
        node_id: Some(node_id.clone()),
        iter: Some(iter),
        payload: None,
    };
    // The synthetic `NodeStarted` probe is the RIGHT probe here, unlike on
    // `node_retry`: `validate_start` answers `Allow` on `live_iter == iter`,
    // and `restart_node` alone may re-spawn a live iteration. Do not narrow it
    // to `run_accepts_lifecycle`.
    match transition_guard::validate_transition(projected.as_ref(), &restart_probe) {
        transition_guard::Verdict::Allow => {}
        transition_guard::Verdict::Reject { reason } => {
            // ONE slug, the guard's prose in `message`. Three of the guard's
            // reasons land here and are deliberately NOT discriminated: see
            // `RestartRefusal::RestartRejected`.
            let refusal = restart_verdict::RestartRefusal::RestartRejected {
                // Forward the typed cause as prose; the slug stays
                // `restart_refused`, still not discriminated.
                message: reason.to_string(),
                session_killed: false,
            };
            warn!(
                "restart_node rejected for {node_id} iter {iter} in {run_id}: {}",
                refusal.reason()
            );
            return crate::adapters::http::verdicts::restart_response(
                &restart_verdict::RestartVerdict::Refused(refusal),
            );
        }
        // Defensive parity with `force_spawn_node`, which treats the two
        // identically. `validate_start` never returns `NoOp` today — it is
        // `Allow` or `Reject` — so this arm is unreachable in production and
        // no layer-3 test claims to reach it.
        transition_guard::Verdict::NoOp { reason } => {
            info!("restart_node no-op for {node_id} iter {iter} in {run_id}: {reason}");
            return crate::adapters::http::verdicts::restart_response(
                &restart_verdict::RestartVerdict::NoOp { reason },
            );
        }
    }

    // PRE-KILL PROBE 2: does the Run exist at all? `404` WITHOUT a trace —
    // this used to be answered after the kill and the `CommandIssued` append.
    let Some(run_state) = projected else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "run not found" })),
        )
            .into_response();
    };

    // PRE-KILL PROBE 3: the Run's pipeline SNAPSHOT, not the library
    // (ADR-0025 §2) — the same snapshot-first helper `extend_cycle` uses.
    let repo_root = effective_repo_root(&state, &run_state);
    let pipeline_path = resolve_run_pipeline_path(&repo_root, &run_id, &run_state.pipeline_name);
    let Ok(yaml) = std::fs::read_to_string(&pipeline_path) else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "cannot read pipeline" })),
        )
            .into_response();
    };
    let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "cannot parse pipeline" })),
        )
            .into_response();
    };
    let pipeline = parse_result.pipeline;

    // PRE-KILL PROBE 4: is the target in that pipeline? Without the `else`,
    // an unknown `node_id` answered `200 {"ok":true}` after killing a session
    // and appending an audit event for work that never happened.
    let Some(node) = pipeline.nodes.iter().find(|n| n.id == node_id) else {
        let refusal = restart_verdict::RestartRefusal::NodeNotFound {
            node_id: node_id.clone(),
        };
        warn!("restart_node refused in {run_id}: {}", refusal.reason());
        return crate::adapters::http::verdicts::restart_response(
            &restart_verdict::RestartVerdict::Refused(refusal),
        );
    };

    // PRE-KILL PROBE 5: is the sandbox container up? `sandbox_spawn_block()`
    // is pure, so evaluating it here costs nothing and turns a post-kill `200`
    // lie into a pre-kill `409`. Read off the SAME projection as above.
    if let Some(reason) = run_state.sandbox_spawn_block() {
        let refusal = restart_verdict::RestartRefusal::SandboxPrepNotReady {
            message: reason,
            session_killed: false,
        };
        info!("restart_node deferred in {run_id}: {}", refusal.reason());
        return crate::adapters::http::verdicts::restart_response(
            &restart_verdict::RestartVerdict::Refused(refusal),
        );
    }

    // ── PRE-KILL PROBE 6: is the sub-worktree someone else's? (#489-B) ────
    //
    // A pure `git` read. The cost is accepted on ADR-0037 §3's terms: nothing
    // knowable is paid for with a kill. `Absent` / `Reusable` / `Recyclable`
    // all proceed — `ensure_sub_worktree` handles each, and never destroys
    // work in flight.
    // #653/ADR-0060: read the isolation FROZEN on this iteration, not the
    // document's — the re-spawn below will land in the frozen directory,
    // so probing the other one would classify a worktree nobody is about
    // to use (and skip the one that matters).
    let owns_sub_worktree = crate::domain::merge_action::frozen_isolation(&events, &node_id, iter)
        .unwrap_or(node.is_isolated());
    if owns_sub_worktree {
        let sub_wt_dir = pdo_workspace::adapters::git::worktrees::sub_worktree_path(
            &repo_root, &run_id, &node_id, iter,
        );
        let sub_branch =
            pdo_workspace::adapters::git::worktrees::sub_worktree_branch(&run_id, &node_id, iter);
        let pipeline_branch = format!("pdo/run-{run_id}");
        if let pdo_workspace::adapters::git::worktrees::SubWorktreeState::Occupied { detail } =
            pdo_workspace::adapters::git::worktrees::classify_sub_worktree(
                &repo_root,
                &sub_wt_dir,
                &sub_branch,
                &pipeline_branch,
            )
        {
            let refusal = restart_verdict::RestartRefusal::SubWorktreeOccupied { message: detail };
            warn!("restart_node refused in {run_id}: {}", refusal.reason());
            return crate::adapters::http::verdicts::restart_response(
                &restart_verdict::RestartVerdict::Refused(refusal),
            );
        }
    }

    // FROM HERE ON THERE ARE SIDE EFFECTS.

    let session_name = pdo_session::domain::names::node_session_name(&run_id, &node_id, iter);
    // Deliberately a BARE kill, not `reap_node_session`: the helper's pane
    // snapshot would never be served, since `GET …/pane` only serves one for a
    // TERMINAL iteration and a restart leaves the node non-terminal.
    state.tmux().kill(&session_name);
    // Also kill the in-container process tree before the re-spawn, or the old
    // session's process lingers alongside the new one. `sandbox` is immutable
    // over a Run's life, so the pre-kill projection above answers it — no
    // post-kill `reload_run_state` needed.
    run_sandbox::kill_session_best_effort(
        state.docker_cmd_override.as_deref().unwrap_or("docker"),
        !run_state.sandbox.is_off(),
        &run_id,
        &session_name,
    );

    let cmd_event = event::Event {
        id: None,
        run_id: run_id.clone(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::CommandIssued,
        node_id: Some(node_id.clone()),
        iter: Some(iter),
        payload: Some(serde_json::json!({
            "command": "restart_node",
            "node_id": node_id,
            "iter": iter,
        })),
    };
    if let Err(e) = append_event(&state, &cmd_event).await {
        error!("failed to append restart_node command event: {e}");
    }

    let worktree_dir = worktree_dir_for_run(&repo_root, &run_id);
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
    let resolved_vars = resolve_run_variables(&pipeline, &events);
    let spawn_ctx = SpawnContext {
        pipeline: &pipeline,
        run_id: &run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: &worktree_dir,
        artifacts_dir: &artifacts_dir,
        resolved_vars: &resolved_vars,
        repo_root: &repo_root,
    };

    // Read the `SpawnOutcome`. It is not `#[must_use]` and must not become
    // one (fire-and-forget schedulers drop it on purpose), but THIS caller has
    // a client waiting on an answer.
    let deps = SpawnDeps::from_state(&state).replacing_live_iteration();
    let outcome = spawn_node(deps, &spawn_ctx, node, iter).await;
    let verdict = match outcome {
        SpawnOutcome::Spawned {
            reused_sub_worktree,
            base_sha,
            interrupted_git_ops,
        } => restart_verdict::RestartVerdict::Spawned {
            node_id: node_id.clone(),
            iter,
            reused_sub_worktree,
            base_sha,
            interrupted_git_ops,
        },
        // ADR-0037 §2: a `2xx`, and NOT a `noop` — a `NodeWaiting` was
        // appended and `retry_waiting_nodes` genuinely picks the node back up.
        SpawnOutcome::Throttled => restart_verdict::RestartVerdict::Waiting {
            reason: format!(
                "node {node_id} iter {iter} is queued behind the session cap: it will \
                 spawn when a slot frees — do not re-issue"
            ),
        },
        // Both of these are races: the head probes above evaluated the same
        // predicates and passed, then `spawn_node` re-evaluated them against
        // a fresher projection. The session is already dead, hence
        // `session_killed`.
        SpawnOutcome::Deferred { reason } => restart_verdict::RestartVerdict::Refused(
            restart_verdict::RestartRefusal::SandboxPrepNotReady {
                message: reason,
                session_killed: true,
            },
        ),
        SpawnOutcome::Refused { reason } => restart_verdict::RestartVerdict::Refused(
            restart_verdict::RestartRefusal::RestartRejected {
                message: reason,
                session_killed: true,
            },
        ),
        // A panne, not a verdict → `500`. `run_failed` is re-PROJECTED, never
        // guessed: the producers of `Failed` disagree about what they append,
        // and a `500` routes the CLI toward `pdo fail` — catastrophic advice if
        // `RunFailed` is already on the log.
        SpawnOutcome::Failed { reason } => {
            let run_failed = reload_run_state(&state, &run_id)
                .await
                .is_some_and(|(_, s)| s.status == state::RunStatus::Failed);
            restart_verdict::RestartVerdict::Broken {
                message: reason,
                run_failed,
            }
        }
    };

    info!("restart_node: node {node_id} iter {iter} in run {run_id} -> {verdict:?}");
    crate::adapters::http::verdicts::restart_response(&verdict)
}
