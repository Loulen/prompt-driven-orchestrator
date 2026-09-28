//! The routes of the Run's surfaces: create (`POST /runs`, JSON or multipart),
//! the list and its reapable part, forget, the children, the artifacts, the
//! refs and the file at a ref, the source drift, the Run's own pipeline (read,
//! exported, edited), the edit of its repositories and its Pipeline Manager.
//! Each translates the request and hands it to its use case; a mutation's use
//! case receives the Actor `identify` set.

pub mod attachments;
mod multipart;

use std::sync::Arc;

use axum::extract::{FromRequest, Json, Multipart, Path, Query, RawQuery, Request, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Extension;
use pdo_kernel::attributes::AttributeFilter;
use pdo_kernel::Actor;

use crate::adapters::env::resolve_max_attachments_mb;
use crate::application::artifact::ArtifactQuery;
use crate::application::create_run::{
    reject_unknown_create_run_fields, CreateRunRequest, PatchRunReposRequest,
};
use crate::application::list::reapable::ReapableQuery;
use crate::application::refs::FileAtRefQuery;
use crate::application::run_pipeline::SaveRunPipelineRequest;
use crate::application::session_claim::session_claim_from_headers;
use crate::Runs;

fn bad_request(message: String) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": message })),
    )
        .into_response()
}

/// `POST /runs`. The multipart body is bounded by the `max_attachments_mb`
/// setting, enforced on the attachment parts (read fresh per request) — not
/// by a static limit frozen at router build; the JSON body keeps its own
/// 10 MB cap.
pub(super) async fn create(
    State(runs): State<Arc<Runs>>,
    Extension(actor): Extension<Actor>,
    req: Request,
) -> Response {
    // ADR-0064: read the session claim BEFORE the body is consumed — identity
    // travels in headers, never in the body (whose parent fields are refused).
    let session = session_claim_from_headers(req.headers());
    let content_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let (parsed, attachments) = if content_type.starts_with("multipart/form-data") {
        // #779: the body limit of this route is the attachment budget, read
        // FRESH from the instance setting. Text fields ride on top of it.
        let stored = runs
            .settings
            .defaults()
            .await
            .and_then(|defaults| defaults.max_attachments_mb);
        let max_attachment_bytes = (resolve_max_attachments_mb(stored) as u64) * 1024 * 1024;
        let multipart = match Multipart::from_request(req, &()).await {
            Ok(m) => m,
            Err(e) => return bad_request(format!("multipart parse error: {e}")),
        };
        match multipart::parse_multipart_create_run(multipart, max_attachment_bytes).await {
            Ok(r) => r,
            Err(refusal) => {
                return (
                    refusal.status,
                    Json(serde_json::json!({ "error": refusal.message })),
                )
                    .into_response();
            }
        }
    } else {
        let body = match axum::body::to_bytes(req.into_body(), 10_000_000).await {
            Ok(b) => b,
            Err(e) => return bad_request(format!("failed to read body: {e}")),
        };
        // Parse to a `Value` FIRST so an unknown top-level field is a named `400`
        // before any effect, not a silently-dropped key.
        let value: serde_json::Value = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => return bad_request(format!("invalid JSON: {e}")),
        };
        if let Err((status, err)) = reject_unknown_create_run_fields(&value) {
            return (status, Json(err)).into_response();
        }
        let parsed: CreateRunRequest = match serde_json::from_value(value) {
            Ok(r) => r,
            Err(e) => return bad_request(format!("invalid JSON: {e}")),
        };
        (parsed, Vec::new())
    };

    runs.create_run(&actor, parsed, attachments, session).await
}

/// `GET /runs[?attr.<id>/<key>=<value>…]`.
pub(super) async fn list(State(runs): State<Arc<Runs>>, RawQuery(query): RawQuery) -> Response {
    runs.list_runs(AttributeFilter::from_query(query.as_deref()))
        .await
}

pub(super) async fn reapable(
    State(runs): State<Arc<Runs>>,
    Query(query): Query<ReapableQuery>,
) -> Response {
    runs.list_reapable(query).await
}

pub(super) async fn forget(
    State(runs): State<Arc<Runs>>,
    Extension(actor): Extension<Actor>,
    Path(run_id): Path<String>,
) -> Response {
    runs.forget_run(&actor, &run_id).await
}

pub(super) async fn children(
    State(runs): State<Arc<Runs>>,
    Path(run_id): Path<String>,
) -> Response {
    runs.run_children(&run_id).await
}

pub(super) async fn file(
    State(runs): State<Arc<Runs>>,
    Path(run_id): Path<String>,
    Query(query): Query<FileAtRefQuery>,
) -> Response {
    runs.file_at_ref(&run_id, query).await
}

pub(super) async fn refs(State(runs): State<Arc<Runs>>, Path(run_id): Path<String>) -> Response {
    runs.run_refs(&run_id).await
}

pub(super) async fn source_drift(
    State(runs): State<Arc<Runs>>,
    Path(run_id): Path<String>,
) -> Response {
    runs.source_drift(&run_id).await
}

pub(super) async fn artifact(
    State(runs): State<Arc<Runs>>,
    Path(run_id): Path<String>,
    Query(query): Query<ArtifactQuery>,
) -> Response {
    runs.artifact(&run_id, query).await
}

pub(super) async fn pipeline(
    State(runs): State<Arc<Runs>>,
    Path(run_id): Path<String>,
) -> Response {
    runs.run_pipeline(&run_id).await
}

pub(super) async fn pipeline_document(
    State(runs): State<Arc<Runs>>,
    Path(run_id): Path<String>,
) -> Response {
    runs.run_pipeline_document(&run_id).await
}

pub(super) async fn pipeline_document_skills(
    State(runs): State<Arc<Runs>>,
    Path(run_id): Path<String>,
) -> Response {
    runs.run_pipeline_skills(&run_id).await
}

pub(super) async fn save_pipeline(
    State(runs): State<Arc<Runs>>,
    Extension(actor): Extension<Actor>,
    Path(run_id): Path<String>,
    Json(request): Json<SaveRunPipelineRequest>,
) -> Response {
    runs.save_run_pipeline(&actor, &run_id, request).await
}

pub(super) async fn edit_repos(
    State(runs): State<Arc<Runs>>,
    Extension(actor): Extension<Actor>,
    Path(run_id): Path<String>,
    Json(request): Json<PatchRunReposRequest>,
) -> Response {
    runs.edit_repos(&actor, &run_id, request).await
}

pub(super) async fn start_manager(
    State(runs): State<Arc<Runs>>,
    Extension(actor): Extension<Actor>,
    Path(run_id): Path<String>,
) -> Response {
    runs.start_manager(&actor, &run_id).await
}

pub(super) async fn stop_manager(
    State(runs): State<Arc<Runs>>,
    Extension(actor): Extension<Actor>,
    Path(run_id): Path<String>,
) -> Response {
    runs.stop_manager(&actor, &run_id).await
}
