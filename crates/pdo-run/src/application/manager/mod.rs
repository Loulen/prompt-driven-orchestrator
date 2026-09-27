//! The Run's Pipeline Manager on demand: start it from the Manager tab (the
//! `manager_enabled` setting gates only the automatic spawn at creation), stop
//! it for cost control. Both are gestures of an actor: they ask the
//! Autorisation d'opération (`run` × `start_manager` / `stop_manager`).

mod spawn;

use std::sync::Arc;

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use pdo_kernel::Actor;
use serde::Serialize;
use tracing::info;
use ts_rs::TS;

pub use spawn::{apply_manager_profile_pin, spawn_manager_session};

use crate::application::events::{emit_run_event, load_projected};
use crate::application::node::prompt::RunNameHint;
use crate::application::operations::on_run;
use crate::application::paths::effective_repo_root;
use crate::domain::event::EventKind;
use crate::domain::state::RunState;
use crate::Runs;

/// Response of `POST /runs/{run_id}/manager/start` (manager on demand).
#[derive(Serialize, TS)]
#[ts(export)]
pub struct ManagerStartResponse {
    pub ok: bool,
    pub session: String,
    /// `true` when THIS call spawned the session; `false` when it already
    /// existed — a double-click Start is an idempotent re-answer, not an error.
    pub created: bool,
}

impl Runs {
/// Start the Run's Pipeline Manager on demand (manager on demand).
///
/// The Manager tab's Start button is ALWAYS available — the `manager_enabled`
/// setting gates only the automatic spawn at Run creation, never this — so the
/// handler is gated on nothing but the Run existing and its worktree still
/// being on disk (an archived Run has none left to work in).
///
/// Idempotency and the racing discipline are the run shell's
/// create-then-verify rule: `session_exists` first (a benign re-answer), spawn,
/// then verify again — a concurrent start may have won the `new-session`, and a
/// spawn failure after our own reservation event must read as an error, not a
/// silent success (the projection ignores `ManagerStarted`; the wire's
/// `has_manager` stays the observed tmux fact).
    pub async fn start_manager(self: &Arc<Self>, actor: &Actor, run_id: &str) -> Response {
        if let Err(denied) = self.authorize(actor, &on_run("start_manager", run_id)) {
            return denied.into_response();
        }
    let state = self;
    let (_, run_state) = match load_projected(state, run_id).await {
        Ok(t) => t,
        Err(resp) => return *resp,
    };
    match ensure_run_manager(state, run_id, &run_state).await {
        Ok(created) => (
            StatusCode::OK,
            Json(ManagerStartResponse {
                ok: true,
                session: pdo_session::domain::names::manager_session_name(run_id),
                created,
            }),
        )
            .into_response(),
        Err((status, body)) => (status, Json(body)).into_response(),
    }
}

/// Stop the Run's Pipeline Manager (manager on demand). Cost control is the
/// point of the feature, so the session must be killable from the panel — not
/// only by the orphan sweep or a manual `tmux kill-session`.
///
/// Stop on a session that is already gone is a calm no-op that still answers
/// `200` ("nothing to stop"), and still writes its `ManagerStopped` event: the
/// projection ignores the kind, so the durable record never lies about state.
    pub async fn stop_manager(self: &Arc<Self>, actor: &Actor, run_id: &str) -> Response {
        if let Err(denied) = self.authorize(actor, &on_run("stop_manager", run_id)) {
            return denied.into_response();
        }
    let state = self;
    // Durable first, kill after — the same order the start path keeps.
    emit_run_event(
        state,
        run_id,
        EventKind::ManagerStopped,
        None,
    )
    .await;

    let session_name = pdo_session::domain::names::manager_session_name(run_id);
    let tmux = state.tmux();
    if !tmux.session_exists(&session_name) {
        return (
            StatusCode::OK,
            Json(serde_json::json!({ "ok": true, "stopped": false })),
        )
            .into_response();
    }
    tmux.kill(&session_name);
    info!("Stopped manager session: {session_name}");
    (
        StatusCode::OK,
        Json(serde_json::json!({ "ok": true, "stopped": true })),
    )
        .into_response()
}
}

/// Make sure the Run's Pipeline Manager session is up, spawning it on demand
/// (manager on demand; #750 reuses it for "send to manager"). `Ok(true)` when
/// THIS call spawned it, `Ok(false)` when it already existed; `Err` carries the
/// status + JSON body the HTTP surfaces answer with (`409` no worktree, `500`
/// the session did not come up).
///
/// Idempotency and the racing discipline are the run shell's create-then-verify
/// rule: `session_exists` first (a benign re-answer), spawn, then verify again —
/// a concurrent start may have won the `new-session`, and a spawn failure after
/// our own reservation event must read as an error, not a silent success (the
/// projection ignores `ManagerStarted`; the wire's `has_manager` stays the
/// observed tmux fact).
pub async fn ensure_run_manager(
    state: &Runs,
    run_id: &str,
    run_state: &RunState,
) -> Result<bool, (StatusCode, serde_json::Value)> {
    let session_name = pdo_session::domain::names::manager_session_name(run_id);
    let tmux = state.tmux();
    if tmux.session_exists(&session_name) {
        return Ok(false);
    }

    // The manager works in the Run's worktree; an archived/cleaned Run has none
    // left, and a session spawned into a dead cwd would only fail at tmux.
    let repo_root = effective_repo_root(state, run_state);
    let worktree_dir =
        pdo_workspace::adapters::git::worktrees::worktree_dir_for_run(&repo_root, run_id);
    if !worktree_dir.exists() {
        return Err((
            StatusCode::CONFLICT,
            serde_json::json!({
                "error": "this run's worktree no longer exists (archived or cleaned) — there is nothing for a manager to work in"
            }),
        ));
    }

    // The create path's name_hint discipline, reconstructed from the projected
    // state: a placeholder name still carries its rename instruction, a named
    // Run is left alone, and a nameless Run derives from its input. The FIRST
    // start (manual or automatic) names the Run best-effort, exactly as before.
    let name_hint = match run_state.name.as_deref() {
        Some(name) if name.starts_with("Untitled run") => {
            RunNameHint::Placeholder
        }
        Some(_) => RunNameHint::UserProvided,
        None => match run_state.input.as_deref() {
            Some(input) if !input.trim().is_empty() => {
                RunNameHint::DeriveFromInput
            }
            _ => RunNameHint::Placeholder,
        },
    };

    spawn_manager_session(
        state,
        run_id,
        &worktree_dir,
        name_hint,
        !run_state.sandbox.is_off(),
    )
    .await;

    // Create-then-verify (the run shell's benign-race rule).
    if !tmux.session_exists(&session_name) {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({
                "error": "the manager tmux session did not come up (see the daemon logs)"
            }),
        ));
    }
    Ok(true)
}
