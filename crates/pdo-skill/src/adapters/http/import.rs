//! Import from a Source (#670): scan, cancel, import, recent sources, and the
//! per-folder rescan / update.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;

use super::dto::{
    ImportSkillsRequest, RescanSkillFolderRequest, ScanSkillSourceRequest,
    UpdateSkillFolderFromSourceRequest,
};
use super::{folder_not_found_response, skill_error_response};
use crate::application::Skills;
use crate::domain::import_error::ImportError;

fn import_error_response(error: ImportError) -> Response {
    use ImportError as E;
    if let E::Skill(inner) = error {
        return skill_error_response(inner);
    }
    let status = match &error {
        E::CloneFailed(_) | E::CloneTimeout => StatusCode::BAD_GATEWAY,
        E::Cancelled => StatusCode::from_u16(499).unwrap_or(StatusCode::BAD_REQUEST),
        E::ScanExpired => StatusCode::GONE,
        E::NotASourceFolder | E::UnknownCandidate(_) | E::UnresolvedCollision(_) => {
            StatusCode::CONFLICT
        }
        E::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
        E::EmptySource | E::InvalidSource(_) | E::LocalNotFound(_) => StatusCode::BAD_REQUEST,
        E::Skill(_) => unreachable!("handled above"),
    };
    (
        status,
        Json(serde_json::json!({
            "error": error.to_string(),
            "code": error.code(),
        })),
    )
        .into_response()
}

/// The daemon's home, which a leading `~` in a local source expands to.
fn daemon_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// `POST /settings/skills/scan` — clone shallow (or open a local folder), walk
/// it for `SKILL.md`s, validate each, and report collisions with the bank.
/// Nothing is written to the bank.
pub(super) async fn scan(
    State(skills): State<Arc<Skills>>,
    Extension(actor): Extension<Actor>,
    Json(req): Json<ScanSkillSourceRequest>,
) -> Response {
    let home = daemon_home();
    match skills
        .scan_source(&actor, &req.scan_id, &req.source, home.as_deref())
        .await
    {
        Ok(report) => Json(serde_json::json!({
            "scan_id": req.scan_id,
            "source": report.outcome.source,
            "commit": report.outcome.commit,
            "candidates": report.candidates,
            "elsewhere": report.outcome.elsewhere,
            "elsewhere_count": report.outcome.elsewhere_count,
        }))
        .into_response(),
        Err(error) => import_error_response(error),
    }
}

pub(super) async fn cancel(
    State(skills): State<Arc<Skills>>,
    Extension(actor): Extension<Actor>,
    AxumPath(scan_id): AxumPath<String>,
) -> Response {
    match skills.cancel_scan(&actor, &scan_id) {
        Ok(cancelled) => Json(serde_json::json!({ "cancelled": cancelled })).into_response(),
        Err(error) => import_error_response(error),
    }
}

/// `POST /settings/skills/import` — write the checked candidates into a Source
/// folder (new or given), with the explicit per-collision choice.
pub(super) async fn import(
    State(skills): State<Arc<Skills>>,
    Extension(actor): Extension<Actor>,
    Json(req): Json<ImportSkillsRequest>,
) -> Response {
    let home = daemon_home();
    match skills
        .import_skills(
            &actor,
            &req.scan_id,
            &req.source,
            home.as_deref(),
            req.folder.unwrap_or_default(),
            req.items,
        )
        .await
    {
        Ok(report) => Json(report).into_response(),
        Err(error) => import_error_response(error),
    }
}

pub(super) async fn recent(State(skills): State<Arc<Skills>>) -> Response {
    match skills.recent_sources().await {
        Ok(sources) => Json(serde_json::json!({ "sources": sources })).into_response(),
        Err(error) => skill_error_response(error),
    }
}

/// `POST /settings/skill-folders/{id}/rescan` — re-clone the folder's source
/// and diff it against the folder's skills. Read-only on the bank.
pub(super) async fn rescan(
    State(skills): State<Arc<Skills>>,
    Extension(actor): Extension<Actor>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<RescanSkillFolderRequest>,
) -> Response {
    match skills.folder(&id).await {
        Ok(Some(_)) => {}
        Ok(None) => return folder_not_found_response(),
        Err(error) => return skill_error_response(error),
    }
    match skills.rescan_folder(&actor, &id, &req.scan_id).await {
        Ok(report) => Json(report).into_response(),
        Err(error) => import_error_response(error),
    }
}

/// `POST /settings/skill-folders/{id}/update` — apply the confirmed diff.
pub(super) async fn update(
    State(skills): State<Arc<Skills>>,
    Extension(actor): Extension<Actor>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<UpdateSkillFolderFromSourceRequest>,
) -> Response {
    match skills.folder(&id).await {
        Ok(Some(_)) => {}
        Ok(None) => return folder_not_found_response(),
        Err(error) => return skill_error_response(error),
    }
    match skills
        .update_from_source(&actor, &id, &req.scan_id, req.items)
        .await
    {
        Ok(report) => Json(report).into_response(),
        Err(error) => import_error_response(error),
    }
}
