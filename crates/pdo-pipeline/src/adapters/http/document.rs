//! The portable document (ADR-0059): export, its skills sidecar (#673), and
//! the import.

use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;
use pdo_skill::domain::sidecar::SidecarSkill;

use super::dto::ImportPipelineDocumentRequest;
use super::pipelines::with_attributes;
use super::{common_response, error};
use crate::application::{PipelineError, Pipelines, SidecarExport};

pub(super) async fn export(
    State(pipelines): State<Arc<Pipelines>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match pipelines.export_document(&id) {
        Ok(document) => (
            [(header::CONTENT_TYPE, "application/yaml; charset=utf-8")],
            document,
        )
            .into_response(),
        Err(e) => document_error(e),
    }
}

pub(super) async fn skills(
    State(pipelines): State<Arc<Pipelines>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match pipelines.document_skills(&id) {
        Ok(sidecar) => skills_sidecar_response(sidecar),
        Err(e) => document_error(e),
    }
}

fn document_error(e: PipelineError) -> Response {
    match e {
        PipelineError::Invalid(e) => {
            error(StatusCode::BAD_REQUEST, format!("invalid pipeline: {e}"))
        }
        PipelineError::Storage(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e),
        e => common_response(&e)
            .unwrap_or_else(|| error(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:?}"))),
    }
}

/// The skills sidecar as a zip of `<pipeline>.skills/<id>/…` read from the
/// bank, or **204** when the pipeline references no skill (nothing to ship —
/// the YAML alone is the document). The Runs' snapshot export answers it too.
pub fn skills_sidecar_response(sidecar: Option<SidecarExport>) -> Response {
    let Some(sidecar) = sidecar else {
        return StatusCode::NO_CONTENT.into_response();
    };
    match pdo_skill::adapters::sidecar_zip::write_zip(&sidecar.pipeline_name, &sidecar.skills) {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, "application/zip".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!(
                        "attachment; filename=\"{}.zip\"",
                        pdo_skill::domain::sidecar::sidecar_dir_name(&sidecar.pipeline_name)
                            .replace('"', "")
                    ),
                ),
                (
                    header::HeaderName::from_static("x-pdo-skills"),
                    sidecar.skills.len().to_string(),
                ),
            ],
            bytes,
        )
            .into_response(),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

pub(super) async fn import(
    State(pipelines): State<Arc<Pipelines>>,
    Extension(actor): Extension<Actor>,
    Json(req): Json<ImportPipelineDocumentRequest>,
) -> Response {
    let sidecar = match decode_sidecar(req.skills_sidecar.as_deref()) {
        Ok(sidecar) => sidecar,
        Err(response) => return *response,
    };
    match pipelines
        .import_document(&actor, &req.document, &sidecar)
        .await
    {
        Ok(imported) => {
            let mut body = serde_json::json!({
                "id": imported.id,
                "scope": "instance",
                "path": imported.path.to_string_lossy(),
                "warnings": imported.warnings,
                "skills": imported.skills,
            });
            let stamped = pipelines.stamp_created(&actor, &imported.id).await;
            with_attributes(&mut body, stamped);
            (StatusCode::CREATED, Json(body)).into_response()
        }
        Err(PipelineError::BadRequest(e)) => error(StatusCode::BAD_REQUEST, e),
        Err(PipelineError::Storage(e)) => error(StatusCode::INTERNAL_SERVER_ERROR, e),
        Err(e) => common_response(&e)
            .unwrap_or_else(|| error(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:?}"))),
    }
}

/// The base64 zip of the request, unpacked; absent or blank is no sidecar.
fn decode_sidecar(encoded: Option<&str>) -> Result<Vec<SidecarSkill>, Box<Response>> {
    let Some(encoded) = encoded.map(str::trim).filter(|e| !e.is_empty()) else {
        return Ok(Vec::new());
    };
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| {
            Box::new(error(
                StatusCode::BAD_REQUEST,
                format!("skills_sidecar: not valid base64: {e}"),
            ))
        })?;
    pdo_skill::adapters::sidecar_zip::read_zip(&bytes).map_err(|e| {
        Box::new(error(
            StatusCode::BAD_REQUEST,
            format!("skills_sidecar: {e}"),
        ))
    })
}
