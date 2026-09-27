//! The workspace's HTTP routes: `/projects*`, `/repos*`, `/fs/browse`,
//! `/settings/provisioning`, `/projects/{id}/provisioning`.
//!
//! The contract is the one the daemon has always spoken: same paths, same
//! bodies, same statuses. The actor of each request comes from the `identify`
//! middleware, which the composition root puts in front of every route.

mod browse;
mod dto;
mod projects;
mod provisioning;
mod repos;

use std::marker::PhantomData;
use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::application::Workspace;

pub use dto::ProjectDto;

/// The settings a Projet carries for the contexts downstream of the workspace
/// (the agentic profile, the skills selection), as those contexts type them.
///
/// The workspace stores them as opaque JSON; the composition root names the
/// real types here, so the wire keeps its exact shape and its validation.
pub trait ProjectSettings: Send + Sync + 'static {
    type AgentChoice: Serialize + DeserializeOwned + TS + Clone + Send + Sync + 'static;
    type Skill: Serialize + DeserializeOwned + TS + Clone + Send + Sync + 'static;

    /// The stored form of a skills selection received on the wire, `None` for
    /// an empty one.
    fn encode_skills(skills: Vec<Self::Skill>) -> Option<String>;

    /// The selection a stored value holds; empty when none or unreadable.
    fn decode_skills(stored: Option<&str>) -> Vec<Self::Skill>;

    fn encode_agent_choice(choice: &Self::AgentChoice) -> String {
        serde_json::to_string(choice).unwrap_or_default()
    }

    /// An unreadable stored choice reads as none (a transparent tier) rather
    /// than failing the whole read.
    fn decode_agent_choice(stored: &str) -> Option<Self::AgentChoice> {
        serde_json::from_str(stored).ok()
    }
}

/// What every handler of the router shares.
pub(crate) struct Http<S> {
    pub(crate) workspace: Arc<Workspace>,
    settings: PhantomData<fn() -> S>,
}

impl<S> Clone for Http<S> {
    fn clone(&self) -> Self {
        Self {
            workspace: self.workspace.clone(),
            settings: PhantomData,
        }
    }
}

/// The workspace's routes, with their state.
pub fn router<S: ProjectSettings>(workspace: Arc<Workspace>) -> Router {
    let http = Http::<S> {
        workspace,
        settings: PhantomData,
    };
    Router::new()
        .route("/repos/branches", get(repos::branches::<S>))
        // #802/ADR-0070: the ONE place the daemon talks to a remote, and only on a
        // human intention (form opened, repo changed, sync clicked). Read-only:
        // fetch + prune, never a push, a merge or a rebase.
        .route("/repos/fetch", post(repos::fetch::<S>))
        .route("/repos/fast-forward", post(repos::fast_forward::<S>))
        .route("/repos/validate", get(repos::validate::<S>))
        // #824 — create a repository from a parent + a name + initial files. The
        // only write-a-new-repo verb in the crate; knows nothing about tours.
        .route("/repos/create", post(repos::create::<S>))
        .route("/repos/recent", get(repos::recent::<S>))
        .route(
            "/repos/provisioning/preview",
            post(provisioning::preview::<S>),
        )
        // Filesystem explorer — not repo-specific (it also serves the settings
        // Dockerfile picker), hence `/fs` rather than `/repos/browse`.
        .route("/fs/browse", get(browse::browse::<S>))
        .route(
            "/settings/provisioning",
            get(provisioning::get_instance::<S>).put(provisioning::put_instance::<S>),
        )
        // Projets (ADR-0046) — the group-header pencil's CRUD and the middle tier of
        // the harness axis.
        .route(
            "/projects",
            get(projects::list::<S>).post(projects::create::<S>),
        )
        .route(
            "/projects/{project_id}",
            get(projects::get::<S>)
                .patch(projects::patch::<S>)
                .delete(projects::delete::<S>),
        )
        .route(
            "/projects/{project_id}/members",
            post(projects::add_member::<S>).delete(projects::remove_member::<S>),
        )
        .route(
            "/projects/{project_id}/provisioning",
            get(provisioning::get_project::<S>).put(provisioning::put_project::<S>),
        )
        .with_state(http)
}

/// `{"error": message}` with `status`.
pub(crate) fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(serde_json::json!({ "error": message.into() }))).into_response()
}

/// An Operation an extension refused: 403, with the reason.
pub(crate) fn denied(denied: pdo_kernel::authorization::Denied) -> Response {
    error(StatusCode::FORBIDDEN, denied.to_string())
}

/// Deserialize a double-`Option` PATCH field so a present `null` maps to
/// `Some(None)` ("clear to NULL"), distinct from absent → `None` ("leave
/// untouched"). serde's default for `Option<Option<T>>` collapses both to
/// `None`; this helper is only invoked when the key is present, which is what
/// recovers the three-way distinction.
pub(crate) fn deserialize_double_option<'de, D, T>(
    deserializer: D,
) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Some(Option::<T>::deserialize(deserializer)?))
}
