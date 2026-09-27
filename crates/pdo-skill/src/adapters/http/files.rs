//! `/settings/skills/{id}/files*`: a skill's reference files (#671).

use std::sync::Arc;

use axum::extract::{FromRequest, Multipart, Path as AxumPath, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;

use super::dto::UploadFromPathRequest;
use super::{skill_error_body, skill_error_response};
use crate::application::{SavedFile, Skills, Upload, Uploaded};
use crate::domain::error::SkillError;
use crate::domain::skill::SKILL_MD;

fn bad_request(message: String, code: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": message, "code": code })),
    )
        .into_response()
}

/// The files of a multipart body, in order, and why the body stopped being
/// read if it did before its end.
async fn multipart_uploads(mut multipart: Multipart) -> (Vec<Upload>, Option<SkillError>) {
    let mut uploads = Vec::new();
    let mut pending_path: Option<String> = None;
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => return (uploads, None),
            // A body over the route limit surfaces here, mid-stream.
            Err(e) => {
                return (
                    uploads,
                    Some(SkillError::Storage(format!("upload interrupted: {e}"))),
                )
            }
        };
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "path" => {
                pending_path = field.text().await.ok().filter(|s| !s.trim().is_empty());
            }
            "file" => {
                let filename = field.file_name().unwrap_or("").to_string();
                let path = pending_path.take().unwrap_or(filename);
                match field.bytes().await {
                    Ok(data) => uploads.push(Upload {
                        path,
                        data: data.to_vec(),
                    }),
                    Err(e) => {
                        let failure = SkillError::Storage(format!("failed to read `{path}`: {e}"));
                        return (uploads, Some(failure));
                    }
                }
            }
            other => {
                let failure = SkillError::InvalidPath(format!(
                    "unknown multipart field `{other}` (accepted: path, file)"
                ));
                return (uploads, Some(failure));
            }
        }
    }
}

/// `POST /settings/skills/{id}/files` — add reference files. Two bodies, one
/// list (#671 design "deux chemins d'upload"):
///
/// - `multipart/form-data`: each `file` part is written under its `filename`
///   (a relative path, sub-folders kept); a `path` text part right before a
///   `file` part overrides that name (browsers flatten `webkitRelativePath`).
/// - JSON `{ "from_path": "/abs/on/host", "path"?: "rel" }`: the daemon copies
///   the file the explorer picked.
///
/// The first refusal stops the batch and is returned with the files already
/// written (`uploaded`), so the popup can mark them and retry the rest.
pub(super) async fn upload(
    State(skills): State<Arc<Skills>>,
    Extension(actor): Extension<Actor>,
    AxumPath(id): AxumPath<String>,
    req: axum::extract::Request,
) -> Response {
    match skills.skill(&id).await {
        Ok(Some(_)) => {}
        Ok(None) => return skill_error_response(SkillError::NotFound),
        Err(error) => return skill_error_response(error),
    }
    let content_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let outcome = if content_type.starts_with("multipart/form-data") {
        let multipart = match Multipart::from_request(req, &()).await {
            Ok(m) => m,
            Err(e) => return bad_request(format!("multipart parse error: {e}"), "bad_multipart"),
        };
        let (uploads, stopped) = multipart_uploads(multipart).await;
        let mut outcome = skills.upload_files(&actor, &id, uploads).await;
        if outcome.failure.is_none() {
            outcome.failure = stopped;
        }
        outcome
    } else {
        let body = match Json::<UploadFromPathRequest>::from_request(req, &()).await {
            Ok(Json(body)) => body,
            Err(e) => {
                return bad_request(
                    format!("expected multipart/form-data or {{ \"from_path\" }}: {e}"),
                    "bad_request",
                )
            }
        };
        let from = std::path::Path::new(&body.from_path);
        match skills
            .copy_host_file(&actor, &id, from, body.path.as_deref())
            .await
        {
            Ok(file) => Uploaded {
                uploaded: vec![file],
                failure: None,
            },
            Err(error) => Uploaded {
                uploaded: Vec::new(),
                failure: Some(error),
            },
        }
    };

    let files = skills.files(&id);
    match outcome.failure {
        None => (
            StatusCode::CREATED,
            Json(serde_json::json!({ "uploaded": outcome.uploaded, "files": files })),
        )
            .into_response(),
        Some(error) => {
            // Keep the status and reason; add what did land, so the client
            // never re-uploads a file that is already there.
            let status = skill_error_response(error.clone()).status();
            let mut json = skill_error_body(&error);
            json["uploaded"] = serde_json::to_value(&outcome.uploaded).unwrap_or_default();
            json["files"] = serde_json::to_value(&files).unwrap_or_default();
            (status, Json(json)).into_response()
        }
    }
}

/// `GET /settings/skills/{id}/files/{*path}` — one file for the plain-text
/// editor: `text` when it decodes as UTF-8, else `text: null, binary: true`
/// (the editor shows "binary file · N KB" instead of the text).
pub(super) async fn read(
    State(skills): State<Arc<Skills>>,
    AxumPath((id, path)): AxumPath<(String, String)>,
) -> Response {
    match skills.read_file(&id, &path).await {
        Ok(file) => Json(serde_json::json!({
            "path": file.path,
            "size": file.size,
            "binary": file.text.is_none(),
            "text": file.text,
        }))
        .into_response(),
        Err(error) => skill_error_response(error),
    }
}

/// `PUT /settings/skills/{id}/files/{*path}` — save the editor's text. Any
/// `Content-Type`; the body is the file. `SKILL.md` goes through the five
/// checks again: a refusal is the same named 400 the paste popup gets, and the
/// file on disk is untouched.
pub(super) async fn write(
    State(skills): State<Arc<Skills>>,
    Extension(actor): Extension<Actor>,
    AxumPath((id, path)): AxumPath<(String, String)>,
    body: String,
) -> Response {
    match skills.save_file(&actor, &id, &path, &body).await {
        Ok(SavedFile::SkillMd { size, skill }) => Json(serde_json::json!({
            "path": SKILL_MD,
            "size": size,
            "skill": skill,
        }))
        .into_response(),
        Ok(SavedFile::Reference(file)) => {
            Json(serde_json::json!({ "path": file.path, "size": file.size })).into_response()
        }
        Err(error) => skill_error_response(error),
    }
}

/// `DELETE /settings/skills/{id}/files/{*path}` — remove one reference file
/// (never `SKILL.md`: 400 `skill_md_reserved`).
pub(super) async fn delete(
    State(skills): State<Arc<Skills>>,
    Extension(actor): Extension<Actor>,
    AxumPath((id, path)): AxumPath<(String, String)>,
) -> Response {
    match skills.delete_file(&actor, &id, &path).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => skill_error_response(error),
    }
}
