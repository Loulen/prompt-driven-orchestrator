//! `POST /runs/{run_id}/nodes/{node_id}/terminal-text` — « Copy & send to
//! terminal »: a text into the input of a node's live session, without Enter.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::application::terminal_text::{
    SendTerminalTextCommand, SendTextError, TERMINAL_TEXT_MAX_BYTES,
};
use crate::Session;

#[derive(Deserialize)]
pub(super) struct TerminalTextRequest {
    #[serde(default)]
    iter: Option<i64>,
    text: String,
}

/// `200` once pasted: the iteration whose session received the text.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct TerminalTextResponse {
    pub ok: bool,
    pub iter: i64,
}

const NOTHING: &str = "nothing written";

/// `200 {ok, iter}` once pasted; every refusal is named and writes nothing.
pub(super) async fn send(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
    AxumPath((run_id, node_id)): AxumPath<(String, String)>,
    body: Result<Json<TerminalTextRequest>, JsonRejection>,
) -> Response {
    // The body is read after the Autorisation d'opération: a malformed body
    // reaches the use case as an empty text, which it refuses once allowed.
    let (iter, text, malformed) = match body {
        Ok(Json(body)) => (body.iter, body.text, None),
        Err(rejection) => (None, String::new(), Some(rejection.body_text())),
    };
    let command = SendTerminalTextCommand {
        run_id: run_id.clone(),
        node_id: node_id.clone(),
        iter,
        text,
    };
    match session.send_terminal_text().execute(&actor, command).await {
        Ok(iter) => (
            StatusCode::OK,
            Json(TerminalTextResponse { ok: true, iter }),
        )
            .into_response(),
        Err(SendTextError::Refused(denied)) => denied.into_response(),
        Err(SendTextError::Empty) => match malformed {
            Some(cause) => refusal(
                StatusCode::BAD_REQUEST,
                "bad_request",
                format!("expected a JSON body `{{\"text\": \"…\"}}` ({cause}); {NOTHING}"),
            ),
            None => refusal(
                StatusCode::BAD_REQUEST,
                "empty_text",
                format!("empty text; {NOTHING}"),
            ),
        },
        Err(SendTextError::TooLong) => refusal(
            StatusCode::BAD_REQUEST,
            "text_too_long",
            format!("text over {} KB; {NOTHING}", TERMINAL_TEXT_MAX_BYTES / 1024),
        ),
        Err(SendTextError::Session(wait, iter)) => {
            let status = if wait.is_not_found() {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::CONFLICT
            };
            let message = wait.gesture_message(&run_id, &node_id, iter);
            refusal(status, wait.slug(), format!("{message}; {NOTHING}"))
        }
        Err(SendTextError::PasteFailed(tmux_session)) => refusal(
            StatusCode::BAD_GATEWAY,
            "paste_failed",
            format!("tmux refused the paste into {tmux_session}; {NOTHING}"),
        ),
    }
}

/// A named refusal: `{error, recoverable, message}`, as the declared wait's.
pub(super) fn refusal(status: StatusCode, slug: &str, message: String) -> Response {
    (
        status,
        Json(serde_json::json!({
            "error": slug,
            "recoverable": true,
            "message": message,
        })),
    )
        .into_response()
}
