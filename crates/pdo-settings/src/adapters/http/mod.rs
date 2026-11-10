//! The HTTP routes of the settings context. The composition root merges
//! [`router`] into the daemon's, behind the `identify` middleware that sets the
//! Actor of every request.
//!
//! ROUTER TRAP: a NEW top-level path prefix also needs its entry in the vite dev
//! proxy whitelist (`frontend/vite.config.ts`).

mod agent_profiles;
mod appearance;
mod harnesses;
mod settings;

use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

pub use agent_profiles::WriteAgentProfileRequest;

use crate::application::SettingsError;
use crate::Settings;

/// The routes of the settings context, with their state, and the caller's
/// Apparence (`/actors/me/appearance`). The staging profiles
/// (`/settings/sandbox-profiles*`), the skills (`/settings/skills*`) and the
/// price sync sit under `/settings` too, as a routing decision only: they are
/// the sandbox, skill and stats contexts' routes.
pub fn router(settings: Arc<Settings>) -> Router {
    Router::new()
        // `GET` answers the per-field {effective, source, stored, env, default}
        // view; `PUT` writes the stored tier, with fail-fast validation.
        .route("/settings", get(settings::get).put(settings::put))
        .route(
            "/settings/harnesses/{name}/refresh",
            post(harnesses::refresh),
        )
        .route(
            "/settings/agent-profiles",
            get(agent_profiles::list).post(agent_profiles::create),
        )
        .route(
            "/settings/agent-profiles/{id}",
            get(agent_profiles::get)
                .put(agent_profiles::update)
                .delete(agent_profiles::delete),
        )
        .route(
            "/settings/agent-profiles/{id}/referents",
            get(agent_profiles::referents),
        )
        // The caller's Apparence, under the `/actors` prefix of the Actors'
        // own routes (already in the vite dev proxy whitelist).
        .route(
            "/actors/me/appearance",
            get(appearance::get).put(appearance::put),
        )
        .with_state(settings)
}

/// A use case's refusal, as the routes have always answered it: a refused
/// Operation is the kernel's `403`, everything else `{"error": …}` under its
/// status.
fn refusal(error: SettingsError) -> Response {
    let (status, message) = match error {
        SettingsError::Refused(denied) => return denied.into_response(),
        SettingsError::Invalid(m) => (StatusCode::BAD_REQUEST, m),
        SettingsError::NotFound(m) => (StatusCode::NOT_FOUND, m),
        SettingsError::Conflict(m) => (StatusCode::CONFLICT, m),
        SettingsError::TooLarge(m) => (StatusCode::PAYLOAD_TOO_LARGE, m),
        SettingsError::Storage(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
    };
    (status, Json(serde_json::json!({ "error": message }))).into_response()
}
