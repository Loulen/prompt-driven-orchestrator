//! `GET`/`PUT /sessions/{session_id}/terminal-zoom` — the Zoom du terminal of
//! a session (CORE-124): what its terminal's text is scaled by on every Poste.
//! A Session hébergée carries one; any other session is at 100 %.

use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_extension_api::DEFAULT_TERMINAL_ZOOM;
use pdo_kernel::Actor;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::application::hosted::HostedZoomError;
use crate::Session;

/// The Zoom du terminal of a session.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct TerminalZoomResponse {
    /// In percent, 50 to 200.
    zoom: u16,
    /// Whether the session carries one (a Session hébergée): only then can it
    /// be set.
    hosted: bool,
}

#[derive(Deserialize)]
pub(super) struct TerminalZoomRequest {
    zoom: u16,
}

/// `403` when the caller may not read the session.
pub(super) async fn read(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
    AxumPath(session_id): AxumPath<String>,
) -> Response {
    match session.hosted().zoom_of(&actor, &session_id) {
        Ok(Some(zoom)) => Json(TerminalZoomResponse { zoom, hosted: true }).into_response(),
        Ok(None) => Json(TerminalZoomResponse {
            zoom: DEFAULT_TERMINAL_ZOOM,
            hosted: false,
        })
        .into_response(),
        Err(denied) => denied.into_response(),
    }
}

/// The Operation `session` × `zoom`; `400` for a zoom out of range, `404` for
/// a session that is not hosted.
pub(super) async fn set(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
    AxumPath(session_id): AxumPath<String>,
    Json(body): Json<TerminalZoomRequest>,
) -> Response {
    match session.hosted().zoom_by_id(&actor, &session_id, body.zoom) {
        Ok(hosted) => Json(TerminalZoomResponse {
            zoom: hosted.terminal_zoom,
            hosted: true,
        })
        .into_response(),
        Err(HostedZoomError::Refused(denied)) => denied.into_response(),
        Err(refusal) => super::error(
            StatusCode::from_u16(refusal.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            refusal.to_string(),
        ),
    }
}
