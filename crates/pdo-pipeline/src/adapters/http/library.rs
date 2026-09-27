//! `/library*` and `/nodes/parse`: the node library, a pasted node, and the
//! import of a Claude Code workflow.

use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;

use super::dto::{ImportWorkflowRequest, ParseNodeRequest, SaveToLibraryRequest};
use super::{common_response, error};
use crate::application::{PipelineError, Pipelines, WorkflowImportError};
use crate::domain::node_spec::parse_node_spec;

pub(super) async fn list(State(pipelines): State<Arc<Pipelines>>) -> Response {
    Json(pipelines.library()).into_response()
}

pub(super) async fn save(
    State(pipelines): State<Arc<Pipelines>>,
    Extension(actor): Extension<Actor>,
    Json(req): Json<SaveToLibraryRequest>,
) -> Response {
    // #655: stamped before saving AND before answering, so the 201 body shows
    // the choice the library recorded rather than the silence the client sent.
    match pipelines.save_to_library(&actor, req.into()) {
        Ok(entry) => (StatusCode::CREATED, Json(&entry)).into_response(),
        Err(PipelineError::Storage(e)) => error(StatusCode::INTERNAL_SERVER_ERROR, e),
        Err(e) => library_error(e),
    }
}

pub(super) async fn delete(
    State(pipelines): State<Arc<Pipelines>>,
    Extension(actor): Extension<Actor>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    match pipelines.delete_from_library(&actor, &name) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(PipelineError::Storage(e)) => error(StatusCode::INTERNAL_SERVER_ERROR, e),
        Err(e) => library_error(e),
    }
}

/// The library's own 404 names an entry, not a pipeline.
fn library_error(e: PipelineError) -> Response {
    match e {
        PipelineError::NotFound => (StatusCode::NOT_FOUND, "entry not found").into_response(),
        e => common_response(&e)
            .unwrap_or_else(|| error(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:?}"))),
    }
}

pub(super) async fn instantiate(
    State(pipelines): State<Arc<Pipelines>>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    let Some(entry) = pipelines.library_entry(&name) else {
        return (StatusCode::NOT_FOUND, "entry not found").into_response();
    };
    let prompt = entry.prompt.clone();
    Json(serde_json::json!({
        "spec": {
            "name": entry.name,
            "type": entry.node_type,
            "inputs": entry.inputs,
            "outputs": entry.outputs,
            "interactive": entry.interactive,
            // Carry model + effort, or instantiating drops the overrides and
            // the fresh node reads `diverged` at once.
            "model": entry.model,
            "effort": entry.effort,
            // #655/ADR-0060: restore the entry's workspace exactly. Left
            // out, the canvas would fall back to the type default and an
            // Agent starred in the Run's worktree would silently fork one
            // of its own on every instantiation. `null` for the types that
            // carry no isolation — the canvas leaves the key off those.
            "isolated_worktree": entry.isolated_worktree,
        },
        "prompt": prompt,
    }))
    .into_response()
}

pub(super) async fn parse_node(Json(req): Json<ParseNodeRequest>) -> Response {
    match parse_node_spec(&req.yaml) {
        Ok(body) => Json(body).into_response(),
        Err(msg) => error(StatusCode::BAD_REQUEST, msg),
    }
}

pub(super) async fn import_workflow(
    State(pipelines): State<Arc<Pipelines>>,
    Extension(actor): Extension<Actor>,
    Json(req): Json<ImportWorkflowRequest>,
) -> Response {
    match pipelines.import_workflow(&actor, &req.filename, &req.content) {
        Ok(imported) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "id": imported.id,
                "scope": "instance",
                "path": imported.path.to_string_lossy(),
                "warnings": imported.warnings,
            })),
        )
            .into_response(),
        Err(WorkflowImportError::Untranslatable(e)) => {
            error(StatusCode::BAD_REQUEST, format!("import failed: {e}"))
        }
        Err(WorkflowImportError::Publish(e)) => error(StatusCode::BAD_REQUEST, e),
        Err(WorkflowImportError::Pipeline(e)) => common_response(&e)
            .unwrap_or_else(|| error(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:?}"))),
    }
}
