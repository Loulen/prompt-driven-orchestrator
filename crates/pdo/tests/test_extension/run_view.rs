//! A Run as the test extension sees it through its `Host` (API 0.12): the
//! core's fixture for the Presences and the cost of a Run.
//!
//! - `GET /ext/test/runs/<id>/view`: `{"presences": […], "cost": <RunCost |
//!   null>}`, as `Host::presences` and `Host::run_cost` answer them;
//! - `GET /ext/test/act-as?actor=<id>`: signs the browser in as the Actor
//!   `<id>` (the cookie [`ACTOR_COOKIE`], read by `identify` like the
//!   [`ACTOR_HEADER`](super::ACTOR_HEADER)), then back to `/`: two browsers,
//!   two Actors;
//! - its bundle's `PeopleMarks`, at `run/item/people`: one mark per Presence
//!   of the row's Run, named as the extensions name the Actor.

use pdo::extension_api::axum::extract::{Path, Query};
use pdo::extension_api::axum::http::{header, StatusCode};
use pdo::extension_api::axum::response::{IntoResponse, Response};
use pdo::extension_api::axum::routing::get;
use pdo::extension_api::axum::{Json, Router};
use pdo::extension_api::RequestParts;
use serde::Deserialize;
use serde_json::json;

use super::creation::Creation;

/// The cookie naming the Actor of a browser (`/ext/test/act-as`).
pub const ACTOR_COOKIE: &str = "pdo-test-actor";

#[derive(Deserialize)]
struct ActAs {
    actor: String,
}

pub(super) fn routes(creation: Creation) -> Router {
    Router::new()
        .route(
            "/runs/{id}/view",
            get(move |Path(id): Path<String>| {
                let host = creation.host();
                async move {
                    let Some(host) = host else {
                        return (
                            StatusCode::SERVICE_UNAVAILABLE,
                            Json(json!({ "error": "the test extension has no Host yet" })),
                        )
                            .into_response();
                    };
                    let presences = host.presences(&id);
                    let cost = host.run_cost(&id).await;
                    Json(json!({ "presences": presences, "cost": cost })).into_response()
                }
            }),
        )
        .route("/act-as", get(act_as))
}

/// The browser is now `actor` (an id of letters, digits, `-` or `_`).
async fn act_as(Query(ActAs { actor }): Query<ActAs>) -> Response {
    let valid = !actor.is_empty()
        && actor
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !valid {
        return (
            StatusCode::BAD_REQUEST,
            "an actor id is letters, digits, - or _",
        )
            .into_response();
    }
    (
        StatusCode::SEE_OTHER,
        [
            (
                header::SET_COOKIE,
                format!("{ACTOR_COOKIE}={actor}; Path=/; SameSite=Lax"),
            ),
            (header::LOCATION, "/".to_string()),
        ],
    )
        .into_response()
}

/// The Actor id the request's [`ACTOR_COOKIE`] names.
pub fn actor_of_cookie(request: &RequestParts) -> Option<String> {
    request
        .headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, value)| *name == ACTOR_COOKIE && !value.is_empty())
        .map(|(_, value)| value.to_string())
}
