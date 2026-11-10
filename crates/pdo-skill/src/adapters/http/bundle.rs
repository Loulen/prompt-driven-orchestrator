//! `/settings/skills/export` and `/settings/skills/upload`: skills of the bank
//! as a zip, and such a zip back into a bank.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{header, HeaderName, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;

use super::dto::{ExportSkillsRequest, UploadSkillsQuery};
use super::skill_error_response;
use crate::adapters::sidecar_zip::{read_zip, write_zip};
use crate::application::Skills;
use crate::domain::error::SkillError;
use crate::domain::sidecar::sidecar_dir_name;

/// `POST /settings/skills/export` — `{ skill_ids, folder_ids }` → a zip of
/// `<name>.skills/<id>/…` (`SKILL.md` + reference files), named after the
/// folder, the skill, or `skills`. `x-pdo-skills` counts the skills shipped.
pub(super) async fn export(
    State(skills): State<Arc<Skills>>,
    Json(req): Json<ExportSkillsRequest>,
) -> Response {
    let export = match skills.export_skills(&req.skill_ids, &req.folder_ids).await {
        Ok(export) => export,
        Err(error) => return skill_error_response(error),
    };
    let bytes = match write_zip(&export.name, &export.skills) {
        Ok(bytes) => bytes,
        Err(error) => return skill_error_response(SkillError::Storage(error)),
    };
    let file_name = format!("{}.zip", sidecar_dir_name(&export.name));
    (
        [
            (header::CONTENT_TYPE, "application/zip".to_string()),
            (header::CONTENT_DISPOSITION, attachment(&file_name)),
            (
                HeaderName::from_static("x-pdo-skills"),
                export.skills.len().to_string(),
            ),
        ],
        bytes,
    )
        .into_response()
}

/// `POST /settings/skills/upload?name=<file name>` — the zip's bytes as the
/// body. Unknown ids are created (same id) under a root folder named after the
/// file, known ids are kept, taken labels are suffixed: the answer is the
/// sidecar import's report.
pub(super) async fn upload(
    State(skills): State<Arc<Skills>>,
    Extension(actor): Extension<Actor>,
    Query(query): Query<UploadSkillsQuery>,
    body: Bytes,
) -> Response {
    let file_name = query.name.unwrap_or_default();
    match skills
        .upload_bundle(&actor, &file_name, read_zip(&body))
        .await
    {
        Ok(report) => (StatusCode::CREATED, Json(report)).into_response(),
        Err(error) => skill_error_response(error),
    }
}

/// `attachment; filename="…"; filename*=UTF-8''…`: an ASCII fallback, and the
/// exact name for the browsers that read RFC 6266's extended form.
fn attachment(file_name: &str) -> String {
    let fallback = file_name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    let encoded = file_name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect::<String>();
    format!("attachment; filename=\"{fallback}\"; filename*=UTF-8''{encoded}")
}

#[cfg(test)]
mod tests {
    use super::attachment;

    #[test]
    fn the_file_name_travels_in_both_forms() {
        assert_eq!(
            attachment("PDO.skills.zip"),
            "attachment; filename=\"PDO.skills.zip\"; filename*=UTF-8''PDO.skills.zip"
        );
        assert_eq!(
            attachment("équipe \"a\".skills.zip"),
            "attachment; filename=\"_quipe _a_.skills.zip\"; \
             filename*=UTF-8''%C3%A9quipe%20%22a%22.skills.zip"
        );
    }
}
