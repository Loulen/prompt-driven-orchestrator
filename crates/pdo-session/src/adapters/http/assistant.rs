//! `/sessions/libassist*` — the library pipeline authoring assistant (ADR-0051).

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::error;
use crate::application::assistant::{OpenAssistantError, SaveFocusedError};
use crate::application::ports::TemplateSaveError;
use crate::domain::libassist::{focus_json, focus_text};
use crate::Session;

/// Response of `POST /sessions/libassist`. Same shape as [`ShellResponse`].
#[derive(Serialize, TS)]
#[ts(export)]
pub struct LibassistResponse {
    ok: bool,
    session: String,
    created: bool,
}

pub(super) async fn open(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
) -> Response {
    match session.open_assistant().execute(&actor).await {
        Ok(opened) => (
            StatusCode::OK,
            Json(LibassistResponse {
                ok: true,
                session: opened.session,
                created: opened.created,
            }),
        )
            .into_response(),
        Err(OpenAssistantError::Refused(denied)) => denied.into_response(),
        Err(OpenAssistantError::Dir(message) | OpenAssistantError::Spawn(message)) => {
            error(StatusCode::INTERNAL_SERVER_ERROR, message)
        }
    }
}

pub(super) async fn close(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
) -> Response {
    match session.close_assistant().execute(&actor) {
        Ok(reaped) => (
            StatusCode::OK,
            Json(serde_json::json!({ "ok": true, "reaped": reaped })),
        )
            .into_response(),
        Err(denied) => denied.into_response(),
    }
}

/// Body of `PUT /sessions/libassist/focus` (ADR-0051 §2).
#[derive(Deserialize)]
pub(super) struct LibassistFocusRequest {
    /// The template being edited, or `null` to clear the focus (the user left
    /// every edit view).
    pipeline_id: Option<String>,
}

pub(super) async fn put_focus(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
    Json(req): Json<LibassistFocusRequest>,
) -> Response {
    match session.declare_focus().execute(&actor, req.pipeline_id) {
        Ok(body) => Json(body).into_response(),
        Err(denied) => denied.into_response(),
    }
}

#[derive(Deserialize)]
pub(super) struct FormatQuery {
    format: Option<String>,
}

/// Read the current focus — the endpoint the assistant's `UserPromptSubmit` hook
/// curls before every message (ADR-0051 §3).
///
/// `?format=text` renders one plain sentence instead of JSON, for the hook: its
/// stdout is appended verbatim to the turn's context, and requiring `jq` on the
/// session's PATH would be a dependency we do not control.
pub(super) async fn get_focus(
    State(session): State<Arc<Session>>,
    Query(q): Query<FormatQuery>,
) -> Response {
    let now = chrono::Utc::now();
    let focus = session.focus().get();
    if q.format.as_deref() == Some("text") {
        return (
            [(
                axum::http::header::CONTENT_TYPE,
                "text/plain; charset=utf-8",
            )],
            focus_text(focus.as_ref()),
        )
            .into_response();
    }
    Json(focus_json(focus.as_ref(), now)).into_response()
}

/// Body of `POST /sessions/libassist/save`: the whole template, and nothing that
/// says *where*.
#[derive(Deserialize)]
pub(super) struct LibassistSaveRequest {
    yaml: String,
    #[serde(default)]
    prompts: HashMap<String, String>,
}

pub(super) async fn save(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
    Json(req): Json<LibassistSaveRequest>,
) -> Response {
    match session
        .save_focused()
        .execute(&actor, &req.yaml, &req.prompts)
    {
        Ok(saved) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "id": saved.pipeline_id,
                "path": saved.path.to_string_lossy(),
            })),
        )
            .into_response(),
        Err(SaveFocusedError::Refused(denied)) => denied.into_response(),
        Err(SaveFocusedError::NoFocus) => error(
            StatusCode::CONFLICT,
            "no pipeline template is open in the UI — ask the user which one to \
             work on instead of guessing a file",
        ),
        // Same gate as `PUT /pipelines/{id}`: a template that does not parse
        // must not reach disk, where it breaks the canvas the user is looking at.
        Err(SaveFocusedError::Save(TemplateSaveError::Invalid {
            error,
            message,
            line,
        })) => {
            let mut body = serde_json::json!({ "error": error, "message": message });
            if let Some(l) = line {
                body["line"] = serde_json::json!(l);
            }
            (StatusCode::BAD_REQUEST, Json(body)).into_response()
        }
        Err(SaveFocusedError::Save(TemplateSaveError::Write(msg))) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": msg.clone(), "message": msg })),
        )
            .into_response(),
    }
}
