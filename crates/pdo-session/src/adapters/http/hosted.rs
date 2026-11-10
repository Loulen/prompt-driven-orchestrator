//! The routes of a Session hébergée named by its id (CORE-125): what the
//! detail of a session reads and does from any Poste, each the extension's
//! gesture on it (the same Opération, the same refusals).
//!
//! - `GET /sessions/{id}/hosted`: the session, and who drives its terminal
//!   (`driving`, the Actor of the Pilote or of the one Poste; `null` while no
//!   one shows it);
//! - `POST /sessions/{id}/terminate`, `POST /sessions/{id}/relaunch`, `PUT
//!   /sessions/{id}/config` (a configuration): the gesture, answering the
//!   session as it now stands;
//! - `GET /sessions/{id}/events`: its journal, oldest first;
//! - `GET /sessions/{id}/live`: its Artefacts live; `GET
//!   /sessions/{id}/live/{*path}`: one, with its content type (`?download`:
//!   as an attachment).
//!
//! A refused Opération answers `403` with the core's body (`{"error":
//! "operation_refused", "message", "reason"}`); any other refusal `{"error"}`
//! with its status: `404` no such Session hébergée, `409` not in a state for
//! it.

use std::sync::Arc;

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_extension_api::{HostedConfig, HostedRefusal};
use pdo_kernel::Actor;
use serde::Deserialize;

use crate::application::hosted::HostedByIdError;
use crate::Session;

/// `GET /sessions/{id}/hosted`.
pub(super) async fn read(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match session.hosted().read_by_id(&actor, &id) {
        Ok(record) => {
            let mut body = serde_json::to_value(&record.session).unwrap_or_default();
            body["driving"] = serde_json::to_value(session.terminals().registry().driver(&id))
                .unwrap_or_default();
            Json(body).into_response()
        }
        Err(refusal) => refused(refusal),
    }
}

/// `POST /sessions/{id}/terminate`.
pub(super) async fn terminate(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    answer(session.hosted().terminate_by_id(&actor, &id))
}

/// `POST /sessions/{id}/relaunch`.
pub(super) async fn relaunch(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    answer(session.hosted().relaunch_by_id(&actor, &id).await)
}

/// `PUT /sessions/{id}/config`: the whole configuration, applied at its next
/// relaunch.
pub(super) async fn configure(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
    AxumPath(id): AxumPath<String>,
    Json(config): Json<HostedConfig>,
) -> Response {
    answer(session.hosted().configure_by_id(&actor, &id, config).await)
}

/// `GET /sessions/{id}/events`.
pub(super) async fn events(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match session.hosted().events_by_id(&actor, &id) {
        Ok(events) => Json(events).into_response(),
        Err(refusal) => refused(refusal),
    }
}

/// `GET /sessions/{id}/live`.
pub(super) async fn live(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let (hosted, actor) = (session.hosted().clone(), actor.clone());
    // It reads the disk.
    let listed = tokio::task::spawn_blocking(move || hosted.live_by_id(&actor, &id))
        .await
        .unwrap_or_else(|e| Err(HostedRefusal::new(500, e.to_string()).into()));
    match listed {
        Ok(artifacts) => Json(artifacts).into_response(),
        Err(refusal) => refused(refusal),
    }
}

#[derive(Deserialize)]
pub(super) struct LiveQuery {
    download: Option<String>,
}

/// `GET /sessions/{id}/live/{*path}`.
pub(super) async fn live_artifact(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
    AxumPath((id, path)): AxumPath<(String, String)>,
    Query(query): Query<LiveQuery>,
) -> Response {
    let (hosted, actor, wanted) = (session.hosted().clone(), actor.clone(), path.clone());
    let read =
        tokio::task::spawn_blocking(move || hosted.live_artifact_by_id(&actor, &id, &wanted))
            .await
            .unwrap_or_else(|e| Err(HostedRefusal::new(500, e.to_string()).into()));
    let bytes = match read {
        Ok(bytes) => bytes,
        Err(refusal) => return refused(refusal),
    };
    let mime = mime_guess::from_path(&path).first_or_octet_stream();
    // Text is read as UTF-8, whatever the platform's default.
    let content_type = if mime.type_() == mime_guess::mime::TEXT {
        format!("{}; charset=utf-8", mime.essence_str())
    } else {
        mime.essence_str().to_string()
    };
    let mut response = bytes.into_response();
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(&content_type) {
        headers.insert(header::CONTENT_TYPE, value);
    }
    // An agent's SVG or HTML is never run in the page's origin.
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "sandbox; default-src 'none'; img-src 'self'; style-src 'unsafe-inline'",
        ),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    if query.download.is_some() {
        let name = path.rsplit('/').next().unwrap_or(&path);
        if let Ok(value) = HeaderValue::from_str(&attachment(name)) {
            headers.insert(header::CONTENT_DISPOSITION, value);
        }
    }
    response
}

/// `Content-Disposition` for downloading a file named `name`: its ASCII form,
/// and its UTF-8 form when it has other characters (RFC 6266, RFC 5987).
fn attachment(name: &str) -> String {
    let ascii: String = name
        .chars()
        .map(|c| match c {
            ' '..='~' if c != '"' && c != '\\' => c,
            _ => '_',
        })
        .collect();
    if ascii == name {
        return format!("attachment; filename=\"{name}\"");
    }
    let encoded: String = name
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect();
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}

fn answer(gesture: Result<pdo_extension_api::HostedSession, HostedByIdError>) -> Response {
    match gesture {
        Ok(session) => Json(session).into_response(),
        Err(refusal) => refused(refusal),
    }
}

/// A refused Opération as every route of the core answers it; any other
/// refusal as `{"error"}` with its status.
fn refused(refusal: HostedByIdError) -> Response {
    match refusal {
        HostedByIdError::Refused(denied) => denied.into_response(),
        HostedByIdError::Refusal(refusal) => super::error(
            StatusCode::from_u16(refusal.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            refusal.message,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::attachment;

    #[test]
    fn a_download_keeps_its_name_in_ascii_and_in_utf_8() {
        assert_eq!(attachment("plan.md"), "attachment; filename=\"plan.md\"");
        assert_eq!(
            attachment("résumé \"v2\".md"),
            "attachment; filename=\"r_sum_ _v2_.md\"; filename*=UTF-8''r%C3%A9sum%C3%A9%20%22v2%22.md"
        );
    }
}
