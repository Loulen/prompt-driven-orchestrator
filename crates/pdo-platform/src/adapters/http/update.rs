//! `/update*` — the version check (#697), « What's new » (#698), and the
//! detached update (#699).

use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;

use super::error;
use crate::application::apply_update::ApplyError;
use crate::application::check_for_update::CheckError;
use crate::application::read_attempt_log::LogError;
use crate::domain::update_status::UpdateStatus;
use crate::Platform;

/// The `GET /update` payload. `installed_version` is the release channel's
/// (#24); `core_version` the core's, equal to it for the vanilla binary.
fn status_json(s: UpdateStatus) -> serde_json::Value {
    serde_json::json!({
        "channel_name": s.channel_name,
        "installed_version": s.installed_version,
        "core_version": s.core_version,
        "latest_version": s.latest_version,
        "newer_available": s.newer_available,
        "checked_at": s.checked_at,
        "source": s.source,
        "source_url": s.source_url,
        "source_page_url": s.source_page_url,
        "check_enabled": s.check_enabled,
        "install_method": s.install_method,
        "manual_command": s.manual_command,
        "supervision": s.supervision,
        "reason": s.reason,
        "last_error": s.last_error,
        "active_runs": s.active_runs,
        "can_apply": s.can_apply,
        "apply_blocked_reason": s.apply_blocked_reason,
        "last_attempt": s.last_attempt,
    })
}

/// `GET /update` — the cached version-check state. Reads the cache, never the
/// network.
pub(super) async fn status(State(platform): State<Arc<Platform>>) -> Response {
    Json(status_json(platform.get_update_status.execute().await)).into_response()
}

/// `POST /update/check` — force a check now. 409 when no release source is
/// declared, when the setting is off (the UI disables the button, a stale client
/// is told) or when a check is already in flight. A failed fetch answers 502
/// **with the refreshed state**: the date moved, the last good version is kept,
/// the error is in `last_error`.
pub(super) async fn check(
    State(platform): State<Arc<Platform>>,
    Extension(actor): Extension<Actor>,
) -> Response {
    match platform.check_for_update.execute(&actor).await {
        Ok(()) => Json(status_json(platform.get_update_status.execute().await)).into_response(),
        Err(CheckError::Refused(denied)) => denied.into_response(),
        Err(CheckError::NoSource(e)) => error(StatusCode::CONFLICT, e),
        Err(CheckError::Off) => error(
            StatusCode::CONFLICT,
            "the update check is off — turn it on to check now",
        ),
        Err(CheckError::InFlight(e)) => error(StatusCode::CONFLICT, e),
        Err(CheckError::Failed(e)) => {
            let mut view = status_json(platform.get_update_status.execute().await);
            view["error"] = serde_json::json!(e);
            (StatusCode::BAD_GATEWAY, Json(view)).into_response()
        }
    }
}

/// `GET /update/changelog` — « What's new ». Never an error: the embedded
/// changelog always answers.
pub(super) async fn changelog(State(platform): State<Arc<Platform>>) -> Response {
    Json(platform.get_changelog.execute().await).into_response()
}

/// `POST /update/apply` — spawn the detached executor, 202 with the attempt at
/// once. 409 with the reason when the method is `unknown`, when the release
/// channel declares no command, or when an attempt is running; 500 when the
/// script cannot be written or spawned (the record then says `failed`, the error
/// is in the log).
pub(super) async fn apply(
    State(platform): State<Arc<Platform>>,
    Extension(actor): Extension<Actor>,
) -> Response {
    match platform.apply_update.execute(&actor).await {
        Ok(started) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "attempt_id": started.attempt_id,
                "status": "running",
                "method": started.method,
                "command": started.command,
                "supervision": started.supervision,
                "log_path": started.log_path,
                "started_at": started.started_at,
                "from_version": started.from_version,
            })),
        )
            .into_response(),
        Err(ApplyError::Refused(denied)) => denied.into_response(),
        Err(ApplyError::Blocked {
            reason,
            method,
            manual_command,
        }) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": reason,
                "install_method": method,
                "manual_command": manual_command,
            })),
        )
            .into_response(),
        Err(ApplyError::NoHome) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "HOME is not set; cannot place the update journal",
        ),
        Err(ApplyError::Failed { error, attempt_id }) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error, "attempt_id": attempt_id })),
        )
            .into_response(),
    }
}

/// `GET /update/attempts/{id}/log` — the executor's journal, `text/plain`.
pub(super) async fn attempt_log(
    State(platform): State<Arc<Platform>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match platform.read_attempt_log.execute(&id) {
        Ok(text) => (
            StatusCode::OK,
            [("content-type", "text/plain; charset=utf-8")],
            text,
        )
            .into_response(),
        Err(LogError::InvalidId) => {
            (StatusCode::BAD_REQUEST, format!("invalid attempt id: {id}")).into_response()
        }
        Err(LogError::NoHome) => {
            (StatusCode::INTERNAL_SERVER_ERROR, "HOME is not set").into_response()
        }
        Err(LogError::NotFound) => (
            StatusCode::NOT_FOUND,
            format!("no log for the update attempt {id}"),
        )
            .into_response(),
        Err(LogError::Unreadable(e)) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}
