//! `POST /runs/{run_id}/nodes/{node_id}/attachments[?iter=N]` — files from the
//! user's machine imported into a node that holds a live session. Multipart,
//! one or more `files` parts, under the instance budget `max_attachments_mb`
//! counted per import (the one of `POST /runs`).
//!
//! Every precondition is checked before anything is written: the use case's
//! (authorization, live session, worktree) before the body is even read, then
//! the budget and the names while it is read in memory. A refusal leaves the
//! Blackboard untouched.

use std::sync::Arc;

use axum::extract::multipart::MultipartRejection;
use axum::extract::{Json, Multipart, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Extension;
use pdo_kernel::Actor;
use serde::{Deserialize, Serialize};
use tracing::info;
use ts_rs::TS;

use crate::adapters::http::runs::attachment_budget_mb;
use crate::adapters::http::runs::attachments::{
    multipart_cut_message, multipart_field_error, sanitize_attachment_filename,
};
use crate::application::import_files::{ImportError, ImportFile, ImportedFile};
use crate::Runs;

const NOTHING: &str = "nothing imported";

#[derive(Deserialize, Default)]
pub(in crate::adapters::http) struct ImportQuery {
    #[serde(default)]
    iter: Option<i64>,
}

/// One file of an import, once written.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct ImportedFileEntry {
    /// The final name: suffixed (`contrat-1.pdf`) when the name was taken.
    pub name: String,
    /// Relative to the Run's worktree root.
    pub path: String,
    pub size: u64,
}

/// `201` of an import: the iteration whose session received the files.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct ImportFilesResponse {
    pub iter: i64,
    pub files: Vec<ImportedFileEntry>,
}

impl From<ImportedFile> for ImportedFileEntry {
    fn from(file: ImportedFile) -> Self {
        Self {
            name: file.name,
            path: file.path,
            size: file.size,
        }
    }
}

/// A named refusal: `{error, recoverable, message}`, the message ending on
/// « nothing imported ».
fn refusal(status: StatusCode, slug: &str, message: impl std::fmt::Display) -> Response {
    (
        status,
        Json(serde_json::json!({
            "error": slug,
            "recoverable": true,
            "message": format!("{message}; {NOTHING}"),
        })),
    )
        .into_response()
}

pub(in crate::adapters::http) async fn import(
    State(runs): State<Arc<Runs>>,
    Extension(actor): Extension<Actor>,
    Path((run_id, node_id)): Path<(String, String)>,
    Query(query): Query<ImportQuery>,
    multipart: Result<Multipart, MultipartRejection>,
) -> Response {
    let target = match runs
        .import_target(&actor, &run_id, &node_id, query.iter)
        .await
    {
        Ok(target) => target,
        Err(ImportError::Refused(denied)) => return denied.into_response(),
        Err(ImportError::Session(wait, iter)) => {
            let status = if wait.is_not_found() {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::CONFLICT
            };
            return refusal(
                status,
                wait.slug(),
                wait.gesture_message(&run_id, &node_id, iter),
            );
        }
        Err(ImportError::WorktreeMissing) => {
            return refusal(
                StatusCode::CONFLICT,
                "run_worktree_missing",
                format!("run {run_id} has no worktree on disk"),
            )
        }
        Err(ImportError::Storage(e)) => {
            return refusal(StatusCode::INTERNAL_SERVER_ERROR, "storage", e)
        }
    };
    let multipart = match multipart {
        Ok(multipart) => multipart,
        Err(e) => {
            return refusal(
                StatusCode::BAD_REQUEST,
                "bad_multipart",
                format!("expected a multipart body with `files` parts ({e})"),
            )
        }
    };
    let max_mb = attachment_budget_mb(&runs).await;
    let files = match read_files(multipart, (max_mb as u64) * 1024 * 1024, max_mb).await {
        Ok(files) => files,
        Err(response) => return *response,
    };
    let written = match target.write(&files) {
        Ok(written) => written,
        Err(e) => {
            return refusal(
                StatusCode::INTERNAL_SERVER_ERROR,
                "write_failed",
                format!(
                    "failed to write into {}: {e}",
                    target.relative_dir().display()
                ),
            )
        }
    };
    info!(
        "Run {run_id}: imported {} file(s) for {node_id} iter-{} into {}",
        written.len(),
        target.iter,
        target.relative_dir().display()
    );
    (
        StatusCode::CREATED,
        Json(ImportFilesResponse {
            iter: target.iter,
            files: written.into_iter().map(ImportedFileEntry::from).collect(),
        }),
    )
        .into_response()
}

/// Every `files` part, in memory, chunk by chunk: a file far over the budget
/// is cut at the line instead of being buffered whole.
async fn read_files(
    mut multipart: Multipart,
    max_bytes: u64,
    max_mb: i64,
) -> Result<Vec<ImportFile>, Box<Response>> {
    let mut files: Vec<ImportFile> = Vec::new();
    let mut total: u64 = 0;
    loop {
        let mut field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(e) => {
                let cause = std::error::Error::source(&e)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| e.to_string());
                return Err(Box::new(refusal(
                    StatusCode::BAD_REQUEST,
                    "upload_interrupted",
                    multipart_cut_message(Some("files"), None, total, &cause),
                )));
            }
        };
        let field_name = field.name().unwrap_or("").to_string();
        if field_name != "files" {
            return Err(Box::new(refusal(
                StatusCode::BAD_REQUEST,
                "unknown_field",
                format!("unknown field `{field_name}`; accepted: files"),
            )));
        }
        let raw_name = field.file_name().unwrap_or("").to_string();
        let name = sanitize_attachment_filename(&raw_name).map_err(|message| {
            Box::new(refusal(
                StatusCode::BAD_REQUEST,
                "invalid_filename",
                message,
            ))
        })?;
        let mut data: Vec<u8> = Vec::new();
        loop {
            match field.chunk().await {
                Ok(Some(chunk)) => {
                    total += chunk.len() as u64;
                    if total > max_bytes {
                        return Err(Box::new(refusal(
                            StatusCode::PAYLOAD_TOO_LARGE,
                            "attachments_too_large",
                            // The upload stops at the line: its real size is
                            // unknown here, so the message names no amount.
                            format!(
                                "attachments exceed the per-import limit of {max_mb} MB (the \
                                 limit was passed while receiving `{name}`); raise \
                                 `max_attachments_mb` in Settings or import less"
                            ),
                        )));
                    }
                    data.extend_from_slice(&chunk);
                }
                Ok(None) => break,
                Err(e) => {
                    return Err(Box::new(refusal(
                        StatusCode::BAD_REQUEST,
                        "upload_interrupted",
                        multipart_field_error("files", Some(&raw_name), total, &e),
                    )))
                }
            }
        }
        files.push(ImportFile { name, data });
    }
    if files.is_empty() {
        return Err(Box::new(refusal(
            StatusCode::BAD_REQUEST,
            "no_files",
            "no file in the import",
        )));
    }
    Ok(files)
}
