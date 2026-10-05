//! The Actors as a person sees them (ADR-0081, 0.8): who the caller is, and
//! the display name the extensions give any Actor.
//!
//! - `GET /actors/me`: `{actor, display}`, the caller's [`Actor`] and its
//!   [`ActorDisplay`] (`null` when no extension names it). The interface's
//!   first call: refused, it carries the page to sign in on.
//! - `POST /actors/display` `{actors: [Actor]}`: `{displays: [ActorDisplay |
//!   null]}`, in the same order.
//!
//! ROUTER TRAP: `/actors` is a top-level prefix of the vite dev proxy whitelist
//! (`frontend/vite.config.ts`).

use std::sync::Arc;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use pdo_extension_api::ActorDisplay;
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::Actor;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The caller, as `GET /actors/me` answers it.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct CurrentActor {
    pub actor: Actor,
    pub display: Option<ActorDisplay>,
}

/// The body of `POST /actors/display`.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct ActorsDisplayRequest {
    pub actors: Vec<Actor>,
}

/// The answer of `POST /actors/display`: one entry per asked Actor, in order.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ActorsDisplayResponse {
    pub displays: Vec<Option<ActorDisplay>>,
}

pub(crate) fn router(host: Arc<dyn ExtensionHost>) -> Router {
    Router::new()
        .route("/actors/me", get(me))
        .route("/actors/display", post(display))
        .with_state(host)
}

async fn me(
    State(host): State<Arc<dyn ExtensionHost>>,
    Extension(actor): Extension<Actor>,
) -> Json<CurrentActor> {
    let display = host.display_name(&actor);
    Json(CurrentActor { actor, display })
}

async fn display(
    State(host): State<Arc<dyn ExtensionHost>>,
    Json(request): Json<ActorsDisplayRequest>,
) -> Json<ActorsDisplayResponse> {
    let displays = request
        .actors
        .iter()
        .map(|actor| host.display_name(actor))
        .collect();
    Json(ActorsDisplayResponse { displays })
}
