//! `GET` and `PUT /actors/me/appearance`: the caller's Apparence, never
//! another Actor's. The body of a `PUT` is taken as bytes: a document too
//! large is a `413`, a wrong shape a `400` naming what is wrong, rather than
//! axum's opaque `422`.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;

use super::refusal;
use crate::domain::appearance::ActorAppearance;
use crate::Settings;

pub(super) async fn get(
    State(settings): State<Arc<Settings>>,
    Extension(actor): Extension<Actor>,
) -> Response {
    match settings.appearance(&actor).await {
        Ok(appearance) => Json(ActorAppearance { appearance }).into_response(),
        Err(e) => refusal(e),
    }
}

pub(super) async fn put(
    State(settings): State<Arc<Settings>>,
    Extension(actor): Extension<Actor>,
    body: Bytes,
) -> Response {
    match settings.set_appearance(&actor, &body).await {
        Ok(document) => Json(ActorAppearance {
            appearance: Some(document),
        })
        .into_response(),
        Err(e) => refusal(e),
    }
}
