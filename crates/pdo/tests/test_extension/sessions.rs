//! The Sessions hébergées of the test extension (API 0.17): the core's
//! fixture for the Host's `open_session`, `hosted_session`, `hosted_sessions`
//! and `terminate_session`.
//!
//! - `POST /ext/test/sessions` opens one on behalf of the caller (the body is
//!   an [`OpenSession`]: `{"key", "kind": "agent"|"shell", "config"?,
//!   "folder"?, "project"?}`) and answers the session (`201`) or the
//!   refusal (`{"error"}`, with its status);
//! - `GET /ext/test/sessions` lists them, `GET /ext/test/sessions/<key>`
//!   reads one;
//! - `DELETE /ext/test/sessions/<key>` terminates it on behalf of the caller.
//!
//! A Session hébergée gets, in its environment, a `PDO_TOKEN` naming who
//! started it ([`token_of`]), which the test extension identifies
//! ([`identify`]): a `pdo` typed in a hosted shell acts as that Actor.

use pdo::extension_api::axum::extract::Path;
use pdo::extension_api::axum::http::StatusCode;
use pdo::extension_api::axum::response::{IntoResponse, Response};
use pdo::extension_api::axum::routing::get;
use pdo::extension_api::axum::{Extension, Json, Router};
use pdo::extension_api::{Actor, HostedRefusal, OpenSession, RequestParts};
use serde_json::{json, Value};

use super::creation::Creation;
use super::ID;

/// The prefix of the token a Session hébergée carries.
pub const SESSION_TOKEN_PREFIX: &str = "test-session.";

/// The token of a Session hébergée started by `actor`: its id for an
/// identified Actor, its kind otherwise.
pub fn token_of(actor: Option<&Actor>) -> String {
    let who = match actor {
        Some(Actor::Identified { id, .. }) => id.clone(),
        Some(other) => other.kind().to_string(),
        None => "nobody".to_string(),
    };
    format!("{SESSION_TOKEN_PREFIX}{who}")
}

/// The Actor a request carrying a session's token is: the one who started the
/// session, identified by the test extension.
pub fn identify(request: &RequestParts) -> Option<Actor> {
    let id = request
        .headers
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")?
        .strip_prefix(SESSION_TOKEN_PREFIX)?;
    (!id.is_empty()).then(|| Actor::Identified {
        namespace: ID.to_string(),
        id: id.to_string(),
    })
}

fn refused(refusal: HostedRefusal) -> Response {
    let status = StatusCode::from_u16(refusal.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json(json!({ "error": refusal.message }))).into_response()
}

fn no_host() -> Response {
    refused(HostedRefusal::unavailable(
        "the test extension has no host yet",
    ))
}

pub(super) fn routes(creation: Creation) -> Router {
    let (open, list, read, terminate) = (
        creation.clone(),
        creation.clone(),
        creation.clone(),
        creation,
    );
    Router::new()
        .route(
            "/sessions",
            get(move || {
                let sessions = list.host().map(|h| h.hosted_sessions()).unwrap_or_default();
                async move { Json(json!(sessions)) }
            })
            .post(
                move |Extension(actor): Extension<Actor>, Json(body): Json<Value>| {
                    let host = open.host();
                    async move {
                        let Some(host) = host else { return no_host() };
                        let open: OpenSession = match serde_json::from_value(body) {
                            Ok(open) => open,
                            Err(e) => {
                                return refused(HostedRefusal::new(400, e.to_string()));
                            }
                        };
                        match host.open_session(&actor, open).await {
                            Ok(session) => {
                                (StatusCode::CREATED, Json(json!(session))).into_response()
                            }
                            Err(refusal) => refused(refusal),
                        }
                    }
                },
            ),
        )
        .route(
            "/sessions/{key}",
            get(move |Path(key): Path<String>| {
                let session = read.host().and_then(|h| h.hosted_session(&key));
                async move {
                    match session {
                        Some(session) => Json(json!(session)).into_response(),
                        None => refused(HostedRefusal::new(404, format!("no session `{key}`"))),
                    }
                }
            })
            .delete(
                move |Extension(actor): Extension<Actor>, Path(key): Path<String>| {
                    let host = terminate.host();
                    async move {
                        let Some(host) = host else { return no_host() };
                        match host.terminate_session(&actor, &key).await {
                            Ok(session) => Json(json!(session)).into_response(),
                            Err(refusal) => refused(refusal),
                        }
                    }
                },
            ),
        )
}
