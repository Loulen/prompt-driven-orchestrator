//! `/settings/skill-folders*`: the bank's folder hierarchy.

use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;

use super::dto::{CreateSkillFolderRequest, UpdateSkillFolderRequest};
use super::{folder_not_found_response, skill_error_response};
use crate::application::Skills;

pub(super) async fn list(State(skills): State<Arc<Skills>>) -> Response {
    match skills.folders().await {
        Ok(folders) => Json(serde_json::json!({ "folders": folders })).into_response(),
        Err(error) => skill_error_response(error),
    }
}

pub(super) async fn create(
    State(skills): State<Arc<Skills>>,
    Extension(actor): Extension<Actor>,
    Json(req): Json<CreateSkillFolderRequest>,
) -> Response {
    match skills
        .create_folder(&actor, &req.name, req.parent_id.as_deref())
        .await
    {
        Ok(folder) => (StatusCode::CREATED, Json(folder)).into_response(),
        Err(error) => skill_error_response(error),
    }
}

pub(super) async fn update(
    State(skills): State<Arc<Skills>>,
    Extension(actor): Extension<Actor>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<UpdateSkillFolderRequest>,
) -> Response {
    match skills.folder(&id).await {
        Ok(Some(_)) => {}
        Ok(None) => return folder_not_found_response(),
        Err(error) => return skill_error_response(error),
    }
    match skills
        .update_folder(
            &actor,
            &id,
            req.name.as_deref(),
            req.parent_id.as_ref().map(|p| p.as_deref()),
        )
        .await
    {
        Ok(folder) => Json(folder).into_response(),
        Err(error) => skill_error_response(error),
    }
}

/// `DELETE /settings/skill-folders/{id}` — the folder's skills and sub-folders
/// move to its parent. Never deletes a skill.
pub(super) async fn delete(
    State(skills): State<Arc<Skills>>,
    Extension(actor): Extension<Actor>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match skills.delete_folder(&actor, &id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => folder_not_found_response(),
        Err(error) => skill_error_response(error),
    }
}
