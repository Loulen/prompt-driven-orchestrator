//! `/sessions` — the status bar's gauges, the OS terminal pop-outs, the run
//! shell.

use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;
use serde::Serialize;
use ts_rs::TS;

use super::{error, run_lookup};
use crate::application::attach::{AttachError, Attached};
use crate::application::open_shell::OpenShellError;
use crate::Session;

#[derive(Serialize, TS)]
#[ts(export)]
pub struct AttachResponse {
    ok: bool,
    session: String,
    terminal: String,
}

/// Response of `POST /sessions/{run_id}/shell` (ADR-0021).
///
/// `created` distinguishes a freshly-spawned shell from a re-attach. The client
/// attaches via the `WS /sessions/<session>/pty` bridge — this endpoint never spawns
/// an OS terminal (ADR-0005: inline xterm.js is primary).
#[derive(Serialize, TS)]
#[ts(export)]
pub struct ShellResponse {
    ok: bool,
    session: String,
    created: bool,
}

/// `GET /sessions` — live NodeRun-session count, configured cap, daemon version,
/// persistent-service health and orphan-sweep gauge, for the bottom status bar.
pub(super) async fn status(State(session): State<Arc<Session>>) -> Response {
    let status = session.read_status().execute().await;
    Json(serde_json::json!({
        "live": status.live,
        "cap": status.cap,
        "version": env!("CARGO_PKG_VERSION"),
        "service": status.service,
        "reaper": {
            "last_sweep_at": status.last_sweep_at,
            "killed": status.killed,
            "killed_for_absent_run": status.killed_for_absent_run,
        },
    }))
    .into_response()
}

pub(super) async fn attach(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
    AxumPath(session_id): AxumPath<String>,
) -> Response {
    attached(session.attach().session(&actor, &session_id).await)
}

pub(super) async fn manager_attach(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
    AxumPath(run_id): AxumPath<String>,
) -> Response {
    attached(session.attach().manager(&actor, &run_id).await)
}

fn attached(result: Result<Attached, AttachError>) -> Response {
    match result {
        Ok(Attached { session, terminal }) => (
            StatusCode::OK,
            Json(AttachResponse {
                ok: true,
                session,
                terminal,
            }),
        )
            .into_response(),
        Err(AttachError::Refused(denied)) => denied.into_response(),
        Err(AttachError::NoManager { session }) => error(
            StatusCode::NOT_FOUND,
            format!("manager session {session} not found"),
        ),
        Err(AttachError::Terminal(message)) => error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}

/// Open (or re-attach) an ad-hoc bash shell in a terminal run's pipeline
/// worktree: `409` unless the run is terminal, non-archived, and its pipeline
/// worktree still exists on disk.
pub(super) async fn open_shell(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
    AxumPath(run_id): AxumPath<String>,
) -> Response {
    match session.open_shell().execute(&actor, &run_id).await {
        Ok(opened) => (
            StatusCode::OK,
            Json(ShellResponse {
                ok: true,
                session: opened.session,
                created: opened.created,
            }),
        )
            .into_response(),
        Err(OpenShellError::Refused(denied)) => denied.into_response(),
        Err(OpenShellError::Run(lookup)) => run_lookup(lookup),
        Err(OpenShellError::NotEligible) => error(
            StatusCode::CONFLICT,
            "run is not shell-eligible (must be terminal, non-archived, worktree present)",
        ),
        Err(OpenShellError::Sandbox(e)) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("sandbox container unavailable: {e}"),
        ),
        Err(OpenShellError::Spawn(message)) => error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}
