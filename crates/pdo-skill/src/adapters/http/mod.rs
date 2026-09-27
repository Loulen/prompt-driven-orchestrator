//! The skill context's HTTP routes: `/settings/skills*` and
//! `/settings/skill-folders*` (#668, #670, #671, ADR-0062).
//!
//! The contract is the one the daemon has always spoken: same paths, same
//! bodies, same statuses. The actor of each request comes from the `identify`
//! middleware, which the composition root puts in front of every route.

mod dto;
mod files;
mod folders;
mod import;
mod skills;

use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;

use crate::application::Skills;
use crate::domain::error::SkillError;

/// Body ceiling of the files routes: a handful of 10 MB files per multipart
/// request. The per-file rule (`MAX_FILE_BYTES`) does the naming.
pub const SKILL_UPLOAD_BODY_LIMIT: usize = 64 * 1024 * 1024;

/// The skill context's routes, with their state.
pub fn router(skills: Arc<Skills>) -> Router {
    Router::new()
        // Banque de skills (#668, ADR-0062): the bank's index + content CRUD, the
        // folder hierarchy, and the referents endpoint the delete dialog reads.
        // Folders sit under their own prefix (`/settings/skill-folders`) rather than
        // `/settings/skills/folders`, so they can never shadow a skill id.
        .route("/settings/skills", get(skills::list).post(skills::create))
        .route(
            "/settings/skills/{id}",
            get(skills::detail)
                .put(skills::update)
                .delete(skills::delete),
        )
        .route("/settings/skills/{id}/referents", get(skills::referents))
        // Import from a Source (#670): scan (clone + walk + collisions), import,
        // and the per-folder rescan / update. Fixed segments sit under
        // `/settings/skills/` but never collide with a skill id (UUIDs).
        .route("/settings/skills/scan", post(import::scan))
        .route(
            "/settings/skills/scan/{scan_id}/cancel",
            post(import::cancel),
        )
        .route("/settings/skills/import", post(import::import))
        .route("/settings/skills/sources/recent", get(import::recent))
        .route("/settings/skill-folders/{id}/rescan", post(import::rescan))
        .route("/settings/skill-folders/{id}/update", post(import::update))
        // Reference files (#671): upload (multipart from the browser, or
        // `{ "from_path" }` from the explorer), read / overwrite as plain text,
        // delete. `{*path}` keeps sub-folders (`examples/login.spec.ts`); the
        // context refuses anything that would leave the skill's folder (400).
        // Own body limit: the default 2 MB would refuse a 4 MB fixture before the
        // per-file 10 MB rule gets to name it.
        .route(
            "/settings/skills/{id}/files",
            post(files::upload).route_layer(axum::extract::DefaultBodyLimit::max(
                SKILL_UPLOAD_BODY_LIMIT,
            )),
        )
        .route(
            "/settings/skills/{id}/files/{*path}",
            get(files::read)
                .put(files::write)
                .delete(files::delete)
                .route_layer(axum::extract::DefaultBodyLimit::max(
                    SKILL_UPLOAD_BODY_LIMIT,
                )),
        )
        .route(
            "/settings/skill-folders",
            get(folders::list).post(folders::create),
        )
        .route(
            "/settings/skill-folders/{id}",
            put(folders::update).delete(folders::delete),
        )
        .with_state(skills)
}

/// The response of a refused write: its status, the reason, its stable code.
/// An Operation an extension refused answers as every context does.
pub fn skill_error_response(error: SkillError) -> Response {
    use SkillError as E;
    if let E::Denied(denied) = error {
        return denied.into_response();
    }
    let status = match &error {
        E::NotFound | E::FileNotFound(_) => StatusCode::NOT_FOUND,
        E::Locked { .. } => StatusCode::FORBIDDEN,
        E::DuplicateName { .. } => StatusCode::CONFLICT,
        E::FileTooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
        E::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
        E::InvalidPath(_)
        | E::SkillMdReserved
        | E::SourceNotAFile(_)
        | E::NoFrontmatter
        | E::MalformedFrontmatter(_)
        | E::MissingName
        | E::NameNotKebabCase(_)
        | E::MissingDescription
        | E::EmptyBody
        | E::EmptyLabel
        | E::FolderNotFound
        | E::EmptyFolderName
        | E::FolderCycle => StatusCode::BAD_REQUEST,
        E::Denied(_) => unreachable!("handled above"),
    };
    (status, Json(skill_error_body(&error))).into_response()
}

/// A refusal's body is [`Denied::body`](pdo_kernel::authorization::Denied::body),
/// the partial failure of an upload included.
fn skill_error_body(error: &SkillError) -> serde_json::Value {
    if let SkillError::Denied(denied) = error {
        return denied.body();
    }
    let mut body = serde_json::json!({
        "error": error.to_string(),
        "code": error.code(),
    });
    if let SkillError::DuplicateName { existing_id, name } = error {
        body["existing_id"] = serde_json::Value::String(existing_id.clone());
        body["existing_name"] = serde_json::Value::String(name.clone());
    }
    body
}

/// A folder addressed by its own route that does not exist is a 404, while an
/// unknown folder *named in a body* (a move target, a parent) stays a 400: the
/// first is "no such resource", the second "your request is wrong".
pub(crate) fn folder_not_found_response() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "error": SkillError::FolderNotFound.to_string(),
            "code": "not_found",
        })),
    )
        .into_response()
}

/// Deserialize a double-`Option` field so a present `null` maps to
/// `Some(None)` ("clear"), distinct from absent → `None` ("leave untouched").
pub(crate) fn deserialize_double_option<'de, D, T>(
    deserializer: D,
) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Some(Option::<T>::deserialize(deserializer)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdo_kernel::authorization::Denied;

    #[test]
    fn a_refusal_answers_the_body_every_context_answers() {
        let denied = Denied {
            reason: "the bank is frozen".into(),
        };
        let error = SkillError::Denied(denied.clone());
        assert_eq!(skill_error_body(&error), denied.body());
        assert_eq!(skill_error_response(error).status(), StatusCode::FORBIDDEN);
    }
}
