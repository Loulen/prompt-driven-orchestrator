//! `recover_node`: recovering an `Interrupted` node (ADR-0049 §3) — re-attach
//! the same session when its harness can resume, restart it with its partial
//! artifacts otherwise.

use super::*;

/// A `recover_node` refusal in the ADR-0035 §3 shape, plus the chosen `mechanism`.
/// `409`, never a `2xx` that would pretend a re-attach happened.
pub(super) fn recover_conflict(
    mechanism: crate::domain::recovery::RecoveryMechanism,
    slug: &str,
    message: &str,
) -> Response {
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({
            "error": slug,
            "recoverable": true,
            "mechanism": mechanism.as_str(),
            "message": message,
        })),
    )
        .into_response()
}

/// Render a re-attach [`pdo_session::domain::pane::ReattachOutcome`] as the `recover_node` response.
/// Only reached on the [`crate::domain::recovery::RecoveryMechanism::Reattach`] branch —
/// the restart-with-artifacts branch forwards `restart_node`'s own response.
pub(super) fn recover_response(
    mechanism: crate::domain::recovery::RecoveryMechanism,
    run_id: &str,
    node_id: &str,
    iter: i64,
    outcome: pdo_session::domain::pane::ReattachOutcome,
) -> Response {
    use pdo_session::domain::pane::ReattachOutcome;
    match outcome {
        ReattachOutcome::Resumed => {
            info!("recover_node: re-attached {node_id} iter {iter} in run {run_id}");
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "ok": true,
                    "mechanism": mechanism.as_str(),
                    "reattached": [{ "node_id": node_id, "iter": iter }],
                })),
            )
                .into_response()
        }
        // A re-attach IS a (re)spawn, so the cap can queue it. A `2xx` and not a
        // no-op: the caller must not re-issue.
        ReattachOutcome::CapReached { live, cap } => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "waiting": true,
                "mechanism": mechanism.as_str(),
                "reason": format!(
                    "session cap reached ({live}/{cap}): the re-attach of {node_id} iter {iter} \
                     is queued — do not re-issue"
                ),
            })),
        )
            .into_response(),
        // Honest `409`s, never a silent success. `CannotResume` is unreachable here
        // (this branch was chosen because the harness CAN resume) but is mapped for
        // exhaustiveness.
        ReattachOutcome::ScriptNode => recover_conflict(
            mechanism,
            "script_not_resumable",
            "a script node runs deterministic bash, not an LLM session — it cannot be \
             re-attached (#248, ADR-0017)",
        ),
        ReattachOutcome::WorkingDirMissing => recover_conflict(
            mechanism,
            "worktree_missing",
            "the node's working directory is gone — nothing to re-enter",
        ),
        ReattachOutcome::CannotResume => recover_conflict(
            mechanism,
            "harness_cannot_resume",
            "the frozen harness declares no resume tail (ADR-0045)",
        ),
        // The mechanism choice above already refuses a gone frozen harness, so this
        // is normally unreachable — mapped for exhaustiveness.
        ReattachOutcome::FrozenHarnessGone { harness } => recover_conflict(
            mechanism,
            "frozen_harness_gone",
            &format!(
                "this node's frozen harness '{harness}' no longer resolves; claude is not \
                 relaunched in its place"
            ),
        ),
        ReattachOutcome::TraceRefused { error } => {
            recover_conflict(mechanism, "reattach_refused", &error)
        }
        ReattachOutcome::ResumeFailed { error } => {
            recover_conflict(mechanism, "reattach_failed", &error)
        }
    }
}

pub(super) async fn recover_node(
    state: Arc<Runs>,
    run_id: String,
    node_id: String,
    iter: i64,
) -> Response {
    // ADR-0049 §3: the mechanism is chosen off the node's FROZEN harness,
    // never the YAML's — re-attach in place when it `can_resume()`, else fall
    // back AUTOMATICALLY to restart-with-artifacts. The fallback is not a
    // second human decision.
    let events = match load_events(&state, &run_id).await {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };
    let Some(run_state) = projection::project(&events) else {
        return (StatusCode::NOT_FOUND, "run not found").into_response();
    };
    // A name that WAS frozen but no longer resolves REFUSES — never a silent
    // `claude` relaunch in this node's worktree. A row with NO frozen harness
    // keeps the `claude` floor, which `can_resume`.
    let harness_home_root = run_sandbox::sandbox_home_roots(&state)
        .map(|(home, _)| home)
        .unwrap_or_default();
    let descriptor =
        match crate::application::node::launch::find_launch_harness(&events, &node_id, iter)
            .as_deref()
        {
            Some(name) => {
                match pdo_harness::adapters::descriptor_file::load(&harness_home_root).resolve(name)
                {
                    Some(d) => d,
                    None => {
                        return recover_conflict(
                            crate::domain::recovery::RecoveryMechanism::Reattach,
                            "frozen_harness_gone",
                            &format!(
                                "this node's frozen harness '{name}' no longer resolves \
                             (embedded name dropped or disk descriptor removed); claude is \
                             not relaunched in its place — restore its descriptor, or \
                             retry the node to start fresh"
                            ),
                        );
                    }
                }
            }
            None => pdo_harness::registry::claude(),
        };
    let mechanism = crate::domain::recovery::choose_recovery(descriptor.can_resume());

    // Audit the intent, naming the mechanism so the automatic fallback is
    // legible in the log (parity with the other arms' `CommandIssued`).
    let cmd_event = event::Event {
        id: None,
        run_id: run_id.clone(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::CommandIssued,
        node_id: Some(node_id.clone()),
        iter: Some(iter),
        payload: Some(serde_json::json!({
            "command": "recover_node",
            "node_id": node_id,
            "iter": iter,
            "mechanism": mechanism.as_str(),
        })),
    };
    if let Err(e) = append_event(&state, &cmd_event).await {
        error!("failed to append recover_node command event: {e}");
    }

    match mechanism {
        crate::domain::recovery::RecoveryMechanism::RestartWithArtifacts => {
            // The automatic fallback: a fresh agent handed the partial
            // artifacts as input, reusing the sub-worktree in place so the
            // partial work is never overwritten. Boxed because `dispatch`
            // recurses here.
            info!(
                "recover_node: {node_id} iter {iter} in {run_id} -> \
                 restart_with_artifacts (harness cannot resume)"
            );
            Box::pin(dispatch(
                state,
                run_id,
                RunCommand::RestartNode { node_id, iter },
            ))
            .await
        }
        crate::domain::recovery::RecoveryMechanism::Reattach => {
            // The optimal path: re-attach the SAME session in place, without
            // re-driving the run. Re-open a parked/terminal Run first, or the
            // resurrection `NodeStarted` append is refused.
            let run_state = match crate::application::reopen::embed_reopen_for_targeted_command(
                &state, &run_id, run_state,
            )
            .await
            {
                Ok(s) => s,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                        .into_response();
                }
            };
            // Re-read the log after the possible reopen so the resurrection
            // trace is faithful to the freshest state.
            let events = match load_events(&state, &run_id).await {
                Ok(e) => e,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                        .into_response();
                }
            };
            let repo_root = effective_repo_root(&state, &run_state);
            let session_name =
                pdo_session::domain::names::node_session_name(&run_id, &node_id, iter);
            let outcome = crate::application::node::reattach::reattach_node_session(
                &state,
                &events,
                &run_state,
                &repo_root,
                &run_id,
                &node_id,
                iter,
                &session_name,
            )
            .await;
            recover_response(mechanism, &run_id, &node_id, iter, outcome)
        }
    }
}
