//! The HTTP and WebSocket routes of the session context. The composition root
//! merges [`router`] into the daemon's, behind the `identify` middleware that sets
//! the Actor of every request.
//!
//! ROUTER TRAP: a NEW top-level path prefix also needs its entry in the vite dev
//! proxy whitelist (`frontend/vite.config.ts`).

mod assistant;
pub mod origin;
mod pane;
mod sessions;
mod terminal;
mod terminal_text;
mod wait;
pub mod ws_log;

use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};

use crate::application::ports::RunLookup;
use crate::Session;

pub use assistant::LibassistResponse;
pub use pane::PaneResponse;
pub use sessions::{AttachResponse, ShellResponse};
pub use terminal_text::TerminalTextResponse;

/// The routes of the session context, with their state.
pub fn router(session: Arc<Session>) -> Router {
    Router::new()
        .route("/sessions", get(sessions::status))
        .route(
            "/runs/{run_id}/nodes/{node_id}/wait-user",
            post(wait::declare),
        )
        .route(
            "/runs/{run_id}/nodes/{node_id}/terminal-text",
            post(terminal_text::send),
        )
        .route("/runs/{run_id}/nodes/{node_id}/pane", get(pane::read))
        .route("/sessions/{session_id}/pty", get(terminal::pty))
        .route("/sessions/{session_id}/attach", post(sessions::attach))
        // ADR-0092: `pdo wait-user` from a Session hébergée.
        .route(
            "/sessions/{session_id}/wait-user",
            post(wait::declare_hosted),
        )
        .route(
            "/sessions/{run_id}/manager/attach",
            post(sessions::manager_attach),
        )
        .route("/sessions/{run_id}/shell", post(sessions::open_shell))
        // The library pipeline authoring assistant (ADR-0051). No pipeline id in
        // the path: there is ONE assistant per Actor, the caller's (CORE-61).
        // `POST` is create-if-absent; `DELETE` reaps it when the user leaves
        // every edit view.
        .route(
            "/sessions/libassist",
            post(assistant::open).delete(assistant::close),
        )
        // Which template the UI has open (ADR-0051 §2). `PUT` doubles as the UI's
        // liveness heartbeat — it is what tells the sweep a human is still editing —
        // and `GET` is what the assistant's `UserPromptSubmit` hook reads.
        .route(
            "/sessions/libassist/focus",
            put(assistant::put_focus).get(assistant::get_focus),
        )
        // The assistant's one write path, and what it reads before (ADR-0088:
        // the template's shared draft, never the file). Deliberately takes no id
        // and no scope: the focus already names the template, and "scope" means
        // `.pdo/pipelines/` on an edit tab but `.pdo/library/pipelines/` in the
        // library store, so any second naming is a way of naming the wrong file.
        .route("/sessions/libassist/save", post(assistant::save))
        .route("/sessions/libassist/draft", get(assistant::draft))
        .with_state(session)
}

/// A JSON error body, `{"error": …}`.
fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(serde_json::json!({ "error": message.into() }))).into_response()
}

/// A Run that could not be read, as every Run-scoped route answers it.
fn run_lookup(lookup: RunLookup) -> Response {
    match lookup {
        // Error bodies are JSON, so a front `.catch(() => null)` on
        // `resp.json()` can read the reason instead of swallowing text.
        RunLookup::Storage(e) => error(StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")),
        RunLookup::NotFound => error(StatusCode::NOT_FOUND, "run not found"),
    }
}
