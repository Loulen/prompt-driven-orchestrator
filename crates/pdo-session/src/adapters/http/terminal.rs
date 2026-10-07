//! `WS /sessions/{session_id}/pty?poste=<id>` — the inline terminal.

use std::sync::Arc;
use std::time::Instant;

use super::origin::check_origin;
use super::ws_log;
use crate::adapters::pty::handle_pty_ws;
use crate::domain::shared_terminal::sanitize_poste;
use crate::Session;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Extension;
use pdo_kernel::Actor;
use serde::Deserialize;

/// Query string of the PTY upgrade.
#[derive(Debug, Default, Deserialize)]
pub(super) struct PtyQuery {
    /// The browser's poste id (#867). Absent or malformed ⇒ a poste of its own.
    poste: Option<String>,
}

pub(super) async fn pty(
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<PtyQuery>,
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let requested = Instant::now();
    let path = format!("/sessions/{session_id}/pty");
    let options = session.options();
    if !check_origin(&headers, options.daemon_port, &options.allowed_ws_origins) {
        // Muted 403s were undiagnosable: once an operator configures a public
        // domain, a typo in PDO_ALLOWED_WS_ORIGINS is the #1 cause of a dead
        // terminal (#564, D8). A warning (not an error): this is nominal
        // protective behaviour, and it is remotely triggerable.
        ws_log::refused(&path, StatusCode::FORBIDDEN, "Origin not allowed", &headers);
        return (StatusCode::FORBIDDEN, "Origin not allowed").into_response();
    }
    if let Err(denied) = session
        .terminals()
        .authorize_attach(&actor, &session_id)
        .await
    {
        let refusal = denied.into_response();
        ws_log::refused(&path, refusal.status(), "attach not authorized", &headers);
        return refusal;
    }

    let poste = sanitize_poste(query.poste.as_deref());
    ws.on_upgrade(move |socket| {
        handle_pty_ws(
            socket,
            session_id,
            poste,
            actor,
            session,
            requested.elapsed(),
        )
    })
}
