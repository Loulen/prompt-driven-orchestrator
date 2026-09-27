//! `/settings/skills` and `/settings/skills/{id}`: the bank's index and each
//! skill's content, plus who selects it.

use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;

use super::dto::{CreateSkillRequest, UpdateSkillRequest};
use super::skill_error_response;
use crate::application::Skills;
use crate::domain::error::SkillError;

/// `GET /settings/skills` — the whole bank in one read: skills, folders, and
/// the disk root the panel's footer names.
pub(super) async fn list(State(skills): State<Arc<Skills>>) -> Response {
    match skills.bank().await {
        Ok(bank) => Json(serde_json::json!({
            "skills": bank.skills,
            "folders": bank.folders,
            "root_path": bank.root_path.display().to_string(),
        }))
        .into_response(),
        Err(error) => skill_error_response(error),
    }
}

pub(super) async fn create(
    State(skills): State<Arc<Skills>>,
    Extension(actor): Extension<Actor>,
    Json(req): Json<CreateSkillRequest>,
) -> Response {
    match skills
        .create_skill(
            &actor,
            &req.content,
            req.name.as_deref(),
            req.folder_id.as_deref(),
        )
        .await
    {
        Ok(skill) => (StatusCode::CREATED, Json(skill)).into_response(),
        Err(error) => skill_error_response(error),
    }
}

/// `GET /settings/skills/{id}` — the row plus its content: the raw `SKILL.md`,
/// the parsed frontmatter (for the read-only table), the body, and the
/// reference files. A skill whose folder vanished from disk still answers with
/// its row and `content: null`, never a 500.
pub(super) async fn detail(
    State(skills): State<Arc<Skills>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let detail = match skills.skill_detail(&id).await {
        Ok(detail) => detail,
        Err(error) => return skill_error_response(error),
    };
    let parsed = detail.parsed.as_ref();
    let mut body = serde_json::to_value(&detail.skill).unwrap_or_default();
    body["content"] = serde_json::to_value(&detail.content).unwrap_or_default();
    // The table is rendered in the author's order: `serde_json` objects do not
    // keep insertion order, so the keys travel separately.
    body["frontmatter"] = parsed
        .map(|p| serde_json::to_value(&p.frontmatter).unwrap_or_default())
        .unwrap_or(serde_json::Value::Null);
    body["frontmatter_keys"] = parsed
        .map(|p| {
            serde_json::Value::Array(
                p.frontmatter
                    .keys()
                    .filter_map(|k| k.as_str().map(|s| serde_json::Value::String(s.to_string())))
                    .collect(),
            )
        })
        .unwrap_or(serde_json::Value::Null);
    body["body"] = parsed
        .map(|p| serde_json::Value::String(p.body.clone()))
        .unwrap_or(serde_json::Value::Null);
    body["files"] = serde_json::to_value(&detail.files).unwrap_or_default();
    body["path"] = serde_json::Value::String(detail.path.display().to_string());
    Json(body).into_response()
}

/// `PUT /settings/skills/{id}` — rename (label only) and/or move.
pub(super) async fn update(
    State(skills): State<Arc<Skills>>,
    Extension(actor): Extension<Actor>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<UpdateSkillRequest>,
) -> Response {
    match skills
        .update_skill(
            &actor,
            &id,
            req.name.as_deref(),
            req.folder_id.as_ref().map(|f| f.as_deref()),
        )
        .await
    {
        Ok(skill) => Json(skill).into_response(),
        Err(error) => skill_error_response(error),
    }
}

pub(super) async fn delete(
    State(skills): State<Arc<Skills>>,
    Extension(actor): Extension<Actor>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match skills.delete_skill(&actor, &id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => skill_error_response(SkillError::NotFound),
        Err(error) => skill_error_response(error),
    }
}

/// `GET /settings/skills/{id}/referents` — who selects this skill, by tier
/// (#669): the instance, the Projets, the Triggers, the pipelines' nodes, and
/// the **not yet started** Runs.
pub(super) async fn referents(
    State(skills): State<Arc<Skills>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let referents = match skills.referents(&id).await {
        Ok(referents) => referents,
        Err(error) => return skill_error_response(error),
    };
    let projects = referents
        .projects
        .iter()
        .map(|p| serde_json::json!({ "id": p.id, "name": p.name }))
        .collect::<Vec<_>>();
    let triggers = referents
        .triggers
        .iter()
        .map(|t| serde_json::json!({ "id": t.id, "name": t.name, "pipeline_id": t.pipeline_id }))
        .collect::<Vec<_>>();
    let pipelines = referents
        .pipelines
        .iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "name": p.name,
                "node_id": p.node_id,
                "scope": p.scope
            })
        })
        .collect::<Vec<_>>();
    let runs = referents
        .runs
        .iter()
        .map(|r| {
            serde_json::json!({
                "run_id": r.run_id,
                "pipeline_name": r.pipeline_name,
                "name": r.name,
            })
        })
        .collect::<Vec<_>>();
    Json(serde_json::json!({
        "skill_id": id,
        "instance": referents.instance,
        "projects": projects,
        "triggers": triggers,
        "pipelines": pipelines,
        "runs": runs
    }))
    .into_response()
}
