//! The Sessions hébergées of the test extension (API 0.17): the core's
//! fixture for the Host's `open_session`, `hosted_session`, `hosted_sessions`,
//! `terminate_session`, `relaunch_session`, `configure_session`,
//! `type_in_session`, `forget_session`, `live_artifacts` and `live_artifact`.
//!
//! - `POST /ext/test/sessions` opens one on behalf of the caller (the body is
//!   an [`OpenSession`]: `{"key", "kind": "agent"|"shell", "config"?,
//!   "folder"?, "project"?}`) and answers the session (`201`) or the
//!   refusal (`{"error"}`, with its status);
//! - `GET /ext/test/sessions` lists them, `GET /ext/test/sessions/<key>`
//!   reads one;
//! - `DELETE /ext/test/sessions/<key>` terminates it on behalf of the caller;
//! - `POST /ext/test/sessions/<key>/relaunch` relaunches it, `PUT
//!   /ext/test/sessions/<key>/config` (a [`HostedConfig`]) configures it,
//!   `POST /ext/test/sessions/<key>/type` (`{"text"}`) lays a text in its
//!   input, `POST /ext/test/sessions/<key>/forget` forgets it, each on behalf
//!   of the caller, answering the session or the refusal;
//! - `GET /ext/test/sessions/<key>/live` lists its Artefacts live, `GET
//!   /ext/test/sessions/<key>/live/<path>` reads one.
//!
//! A Session hébergée gets, in its environment, a `PDO_TOKEN` naming who
//! started it ([`token_of`]), which the test extension identifies
//! ([`identify`]): a `pdo` typed in a hosted shell acts as that Actor.

use pdo::extension_api::axum::extract::Path;
use pdo::extension_api::axum::http::StatusCode;
use pdo::extension_api::axum::response::{IntoResponse, Response};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use pdo::extension_api::axum::routing::{get, post, put};
use pdo::extension_api::axum::{Extension, Json, Router};
use pdo::extension_api::{
    Actor, Host, HostedConfig, HostedGesture, HostedRefusal, OpenSession, RequestParts,
};
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

/// What a route of a gesture answers.
type Answer = Pin<Box<dyn Future<Output = Response> + Send>>;

/// The answer of `gesture`.
async fn answer(gesture: HostedGesture) -> Response {
    match gesture.await {
        Ok(session) => Json(json!(session)).into_response(),
        Err(refusal) => refused(refusal),
    }
}

/// A route that has the host do `gesture` on the session of its path, on
/// behalf of the caller, with the request's body.
fn gesture(
    creation: &Creation,
    gesture: impl Fn(&Arc<dyn Host>, &Actor, &str, Value) -> HostedGesture
        + Clone
        + Send
        + Sync
        + 'static,
) -> impl Fn(Extension<Actor>, Path<String>, Option<Json<Value>>) -> Answer + Clone + Send + Sync + 'static
{
    let creation = creation.clone();
    move |Extension(actor): Extension<Actor>, Path(key): Path<String>, body: Option<Json<Value>>| {
        let host = creation.host();
        let gesture = gesture.clone();
        Box::pin(async move {
            let Some(host) = host else { return no_host() };
            let body = body.map(|Json(body)| body).unwrap_or(Value::Null);
            answer(gesture(&host, &actor, &key, body)).await
        })
    }
}

pub(super) fn routes(creation: Creation) -> Router {
    let (relaunch, configure, typed, forget) = (
        gesture(&creation, |host, actor, key, _| {
            host.relaunch_session(actor, key)
        }),
        gesture(
            &creation,
            |host, actor, key, body| match serde_json::from_value::<HostedConfig>(body) {
                Ok(config) => host.configure_session(actor, key, config),
                Err(e) => Box::pin(std::future::ready(Err(HostedRefusal::new(
                    400,
                    e.to_string(),
                )))),
            },
        ),
        gesture(&creation, |host, actor, key, body| {
            let text = body["text"].as_str().unwrap_or_default().to_string();
            host.type_in_session(actor, key, &text)
        }),
        gesture(&creation, |host, actor, key, _| {
            host.forget_session(actor, key)
        }),
    );
    let (live, artifact) = (creation.clone(), creation.clone());
    let gestures = Router::new()
        .route("/sessions/{key}/relaunch", post(relaunch))
        .route("/sessions/{key}/config", put(configure))
        .route("/sessions/{key}/type", post(typed))
        .route("/sessions/{key}/forget", post(forget))
        .route(
            "/sessions/{key}/live",
            get(move |Path(key): Path<String>| {
                let artifacts = live
                    .host()
                    .map(|h| h.live_artifacts(&key))
                    .unwrap_or_default();
                async move { Json(json!(artifacts)) }
            }),
        )
        .route(
            "/sessions/{key}/live/{*path}",
            get(move |Path((key, path)): Path<(String, String)>| {
                let read = artifact
                    .host()
                    .map(|h| h.live_artifact(&key, &path))
                    .unwrap_or_else(|| Err(HostedRefusal::unavailable("no host yet")));
                async move {
                    match read {
                        Ok(bytes) => bytes.into_response(),
                        Err(refusal) => refused(refusal),
                    }
                }
            }),
        );
    Router::new().merge(gestures).merge(sessions(creation))
}

fn sessions(creation: Creation) -> Router {
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
