//! The agentic profiles' routes: `/settings/agent-profiles*`.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;
use serde::Deserialize;
use ts_rs::TS;

use super::refusal;
use crate::application::agent_profiles::ProfileRequest;
use crate::Settings;

#[derive(Deserialize, TS)]
#[ts(export, optional_fields = nullable, rename = "AgentProfileInput")]
pub struct WriteAgentProfileRequest {
    name: String,
    harness: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    effort: Option<String>,
}

impl From<WriteAgentProfileRequest> for ProfileRequest {
    fn from(req: WriteAgentProfileRequest) -> Self {
        ProfileRequest {
            name: req.name,
            harness: req.harness,
            model: req.model,
            effort: req.effort,
        }
    }
}

pub(super) async fn list(State(settings): State<Arc<Settings>>) -> Response {
    match settings.agent_profiles().await {
        // Through `json!`, as the list always went: each profile's keys sorted.
        Ok(profiles) => Json(serde_json::json!({ "profiles": profiles })).into_response(),
        Err(e) => refusal(e),
    }
}

pub(super) async fn get(State(settings): State<Arc<Settings>>, Path(id): Path<String>) -> Response {
    match settings.agent_profile(&id).await {
        Ok(profile) => Json(profile).into_response(),
        Err(e) => refusal(e),
    }
}

pub(super) async fn create(
    State(settings): State<Arc<Settings>>,
    Extension(actor): Extension<Actor>,
    Json(req): Json<WriteAgentProfileRequest>,
) -> Response {
    match settings.create_agent_profile(&actor, req.into()).await {
        Ok(profile) => (StatusCode::CREATED, Json(profile)).into_response(),
        Err(e) => refusal(e),
    }
}

pub(super) async fn update(
    State(settings): State<Arc<Settings>>,
    Path(id): Path<String>,
    Extension(actor): Extension<Actor>,
    Json(req): Json<WriteAgentProfileRequest>,
) -> Response {
    match settings.update_agent_profile(&actor, &id, req.into()).await {
        Ok(profile) => Json(profile).into_response(),
        Err(e) => refusal(e),
    }
}

pub(super) async fn delete(
    State(settings): State<Arc<Settings>>,
    Path(id): Path<String>,
    Extension(actor): Extension<Actor>,
) -> Response {
    match settings.delete_agent_profile(&actor, &id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => refusal(e),
    }
}

pub(super) async fn referents(
    State(settings): State<Arc<Settings>>,
    Path(id): Path<String>,
) -> Response {
    match settings.agent_profile_referents(&id).await {
        // Through a `Value`, as it always went: every object's keys sorted.
        Ok(referents) => Json(serde_json::to_value(referents).unwrap_or_default()).into_response(),
        Err(e) => refusal(e),
    }
}
