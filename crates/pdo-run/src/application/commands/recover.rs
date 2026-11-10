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

/// What the harness's store says about the conversation a re-attach would resume.
fn conversation_on_disk(
    state: &Runs,
    events: &[event::Event],
    run_state: &state::RunState,
    node_id: &str,
    iter: i64,
) -> crate::domain::recovery::Conversation {
    use crate::application::node::transcript::{locate, StoreRoots, TranscriptSource};
    use crate::domain::recovery::Conversation;
    let Ok((home_root, sandbox_root)) = run_sandbox::sandbox_home_roots(state) else {
        return Conversation::Unknown;
    };
    let roots = StoreRoots {
        home_root,
        sandbox_root,
    };
    let repo_root = effective_repo_root(state, run_state);
    match locate(events, run_state, &repo_root, node_id, iter, &roots) {
        TranscriptSource::File { path, .. } if path.is_file() => Conversation::OnDisk,
        TranscriptSource::File { .. } | TranscriptSource::NotWritten { .. } => {
            Conversation::Missing
        }
        TranscriptSource::Unavailable { .. } => Conversation::Unknown,
    }
}

/// The restart's own answer, plus the mechanism that ran and, for a fallback a
/// resuming harness did not ask for, why. A refusal is forwarded untouched.
async fn with_recovery(
    response: Response,
    mechanism: crate::domain::recovery::RecoveryMechanism,
    reason: Option<&str>,
) -> Response {
    if !response.status().is_success() {
        return response;
    }
    let status = response.status();
    let Ok(bytes) = axum::body::to_bytes(response.into_body(), 1 << 20).await else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "error: unreadable restart response",
        )
            .into_response();
    };
    let mut body = match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(serde_json::Value::Object(map)) => map,
        _ => return (status, bytes).into_response(),
    };
    body.insert("mechanism".into(), mechanism.as_str().into());
    if let Some(reason) = reason {
        body.insert("reason".into(), reason.into());
    }
    (status, Json(serde_json::Value::Object(body))).into_response()
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
    // Both mechanisms start the iteration again, so the guard a start meets is
    // asked first, against a preview of the embedded re-open: a recovery it
    // refuses (an iteration already completed) leaves the Run as it was, with
    // neither the re-open nor the audit below written.
    let started_again = event::Event {
        id: None,
        run_id: run_id.clone(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event::EventKind::NodeStarted,
        node_id: Some(node_id.clone()),
        iter: Some(iter),
        payload: None,
    };
    let preview = crate::application::reopen::preview_reopen(&events, run_state.clone());
    if let transition_guard::Verdict::Reject { reason } =
        transition_guard::validate_transition(Some(&preview), &started_again)
    {
        let message = reason.to_string();
        warn!("recover_node refused for {node_id} iter {iter} in {run_id}: {message}");
        return recover_conflict(
            crate::domain::recovery::RecoveryMechanism::Reattach,
            "recover_refused",
            &message,
        );
    }
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
    // A resume of a conversation that was never written dies at once (`claude
    // --resume` answers « No conversation found » and exits): the node is
    // restarted instead, and the response says why.
    let conversation = conversation_on_disk(&state, &events, &run_state, &node_id, iter);
    let mechanism = crate::domain::recovery::choose_recovery(descriptor.can_resume(), conversation);
    let reason = crate::domain::recovery::fallback_reason(descriptor.can_resume(), conversation);

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
            "reason": reason,
        })),
    };
    if let Err(e) = append_event(&state, &cmd_event).await {
        error!("failed to append recover_node command event: {e}");
    }

    match mechanism {
        crate::domain::recovery::RecoveryMechanism::RestartWithArtifacts => {
            // The automatic fallback: a fresh agent handed the partial
            // artifacts as input, reusing the sub-worktree in place so the
            // partial work is never overwritten.
            info!(
                "recover_node: {node_id} iter {iter} in {run_id} -> restart_with_artifacts ({})",
                reason.unwrap_or("harness cannot resume")
            );
            // On its own task, not polled inside this one: nested in the
            // recover arm of `dispatch`, a second `dispatch` overflowed the stack
            // of a debug build — the fallback had never been exercised.
            let restarted = match tokio::spawn(super::restart::restart_node(
                state, run_id, node_id, iter,
            ))
            .await
            {
                Ok(response) => response,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                        .into_response();
                }
            };
            with_recovery(restarted, mechanism, reason).await
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
