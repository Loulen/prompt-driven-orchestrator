//! `/pipelines` and `/pipelines/{id}`: the registry and one pipeline.

use std::sync::Arc;

use axum::extract::{Path as AxumPath, RawQuery, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::attributes::{AttributeFilter, Attributes};
use pdo_kernel::Actor;

use super::dto::{
    CreatePipelineRequest, PipelineListEntry, RenamePipelineRequest, SavePipelineRequest,
};
use super::{common_response, error, error_with_message, structured_parse_error};
use crate::application::{Created, PipelineError, Pipelines, RenameOutcome};
use crate::domain::pipeline::PipelineDef;
use crate::domain::validation::PipelineValidation;

/// `GET /pipelines[?attr.<id>/<key>=<value>…]`.
pub(super) async fn list(
    State(pipelines): State<Arc<Pipelines>>,
    RawQuery(query): RawQuery,
) -> Response {
    let filter = AttributeFilter::from_query(query.as_deref());
    match pipelines.list_carrying(filter).await {
        Ok(list) => {
            let mut validations = pipelines.validations().await;
            Json(
                list.into_iter()
                    .map(|(summary, attributes)| {
                        let validation = validations.remove(&summary.id).unwrap_or_default();
                        PipelineListEntry::from((summary, attributes, validation))
                    })
                    .collect::<Vec<_>>(),
            )
            .into_response()
        }
        Err(PipelineError::Storage(e)) => error(StatusCode::INTERNAL_SERVER_ERROR, e),
        Err(e) => fallback(e),
    }
}

pub(super) async fn detail(
    State(pipelines): State<Arc<Pipelines>>,
    AxumPath(pipeline_id): AxumPath<String>,
) -> Response {
    let detail = match pipelines.detail(&pipeline_id) {
        Ok(detail) => detail,
        Err(PipelineError::Invalid(e)) => {
            return error(StatusCode::BAD_REQUEST, format!("parse error: {e}"))
        }
        Err(e) => return fallback(e),
    };
    let pipeline = match pipeline_json_with_node_provisioning(&detail.pipeline, &detail.yaml) {
        Ok(pipeline) => pipeline,
        Err(e) => {
            return error(
                StatusCode::BAD_REQUEST,
                format!("parse node provisioning: {e}"),
            )
        }
    };
    Json(serde_json::json!({
        "id": detail.id,
        "scope": "instance",
        "path": detail.path.to_string_lossy(),
        "yaml": detail.yaml,
        "pipeline": pipeline,
        "prompts": detail.prompts,
        "diagnostics": detail.diagnostics,
    }))
    .into_response()
}

/// The parsed pipeline as JSON, each node carrying the `provisioning` rules
/// its YAML declares (the parsed [`PipelineDef`] does not keep them).
fn pipeline_json_with_node_provisioning(
    pipeline: &PipelineDef,
    yaml: &str,
) -> anyhow::Result<serde_json::Value> {
    let mut json = serde_json::to_value(pipeline)?;
    let source: serde_yaml::Value = serde_yaml::from_str(yaml)?;
    let Some(source_nodes) = source.get("nodes").and_then(serde_yaml::Value::as_sequence) else {
        return Ok(json);
    };
    let Some(response_nodes) = json
        .get_mut("nodes")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return Ok(json);
    };

    for source_node in source_nodes {
        let Some(node_id) = source_node.get("id").and_then(serde_yaml::Value::as_str) else {
            continue;
        };
        let Some(raw_rules) = source_node.get("provisioning") else {
            continue;
        };
        let rules: pdo_workspace::domain::provisioning::ProvisioningRules =
            serde_yaml::from_value(raw_rules.clone())?;
        if let Some(response_node) = response_nodes
            .iter_mut()
            .find(|node| node.get("id").and_then(serde_json::Value::as_str) == Some(node_id))
        {
            response_node["provisioning"] = serde_json::to_value(rules)?;
        }
    }
    Ok(json)
}

pub(super) async fn create(
    State(pipelines): State<Arc<Pipelines>>,
    Extension(actor): Extension<Actor>,
    Json(req): Json<CreatePipelineRequest>,
) -> Response {
    match pipelines.create(&actor, &req.name) {
        Ok(created) => {
            let stamped = pipelines.settle_created(&actor, &created.id).await;
            created_response(created, stamped)
        }
        Err(PipelineError::NameRequired) => error(StatusCode::BAD_REQUEST, "name is required"),
        Err(PipelineError::AlreadyExists) => error(StatusCode::CONFLICT, "pipeline already exists"),
        Err(PipelineError::Storage(e)) => error(StatusCode::INTERNAL_SERVER_ERROR, e),
        Err(e) => fallback(e),
    }
}

pub(super) async fn save(
    State(pipelines): State<Arc<Pipelines>>,
    Extension(actor): Extension<Actor>,
    AxumPath(pipeline_id): AxumPath<String>,
    Json(req): Json<SavePipelineRequest>,
) -> Response {
    match pipelines
        .save(&actor, &pipeline_id, &req.yaml, &req.prompts)
        .await
    {
        Ok(saved) => (
            StatusCode::OK,
            Json(serde_json::json!({ "ok": true, "id": saved.id, "renamed": saved.renamed })),
        )
            .into_response(),
        Err(PipelineError::Invalid(e)) => structured_parse_error("invalid YAML", &e),
        Err(e) => move_refusal(e),
    }
}

pub(super) async fn rename(
    State(pipelines): State<Arc<Pipelines>>,
    Extension(actor): Extension<Actor>,
    AxumPath(pipeline_id): AxumPath<String>,
    Json(req): Json<RenamePipelineRequest>,
) -> Response {
    let body = match pipelines.rename(&actor, &pipeline_id, &req.name).await {
        Ok(RenameOutcome::Unchanged { id, name }) => {
            serde_json::json!({ "ok": true, "id": id, "name": name, "renamed": false })
        }
        Ok(RenameOutcome::InPlace { id, name }) => {
            serde_json::json!({ "ok": true, "id": id, "name": name, "renamed": true })
        }
        Ok(RenameOutcome::Moved { id, name, path }) => {
            pipelines.carry_attributes(&pipeline_id, &id).await;
            serde_json::json!({
            "ok": true,
            "id": id,
            "name": name,
            "path": path.to_string_lossy(),
            "renamed": true,
            })
        }
        Err(PipelineError::NameRequired) => {
            return error_with_message(
                StatusCode::BAD_REQUEST,
                "name is required".into(),
                "name is required".into(),
            )
        }
        Err(PipelineError::Invalid(e)) => return structured_parse_error("invalid pipeline", &e),
        Err(e) => return move_refusal(e),
    };
    (StatusCode::OK, Json(body)).into_response()
}

/// The refusals a save and a rename share: a taken identity, Runs in flight,
/// a document with no `name:` line, a failed write or move.
fn move_refusal(error: PipelineError) -> Response {
    match error {
        PipelineError::NameTaken(conflict) => error_with_message(
            StatusCode::CONFLICT,
            format!("Cannot rename: {conflict}"),
            conflict,
        ),
        PipelineError::ActiveRuns(count) => {
            let message = format!(
                "Cannot rename while runs are active: {count} active run(s) reference this pipeline"
            );
            error_with_message(StatusCode::CONFLICT, message.clone(), message)
        }
        PipelineError::NoNameLine => {
            let message = "pipeline has no top-level 'name:' key".to_string();
            error_with_message(StatusCode::INTERNAL_SERVER_ERROR, message.clone(), message)
        }
        PipelineError::Storage(e) => {
            error_with_message(StatusCode::INTERNAL_SERVER_ERROR, e.clone(), e)
        }
        e => fallback(e),
    }
}

pub(super) async fn duplicate(
    State(pipelines): State<Arc<Pipelines>>,
    Extension(actor): Extension<Actor>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match pipelines.duplicate(&actor, &id) {
        Ok(created) => {
            let stamped = pipelines.settle_created(&actor, &created.id).await;
            created_response(created, stamped)
        }
        Err(PipelineError::Invalid(e)) => error(
            StatusCode::BAD_REQUEST,
            format!("invalid source pipeline: {e}"),
        ),
        Err(PipelineError::Storage(e)) => error(StatusCode::INTERNAL_SERVER_ERROR, e),
        Err(e) => fallback(e),
    }
}

/// `PUT /pipelines/{id}/validation`: replace the Projets the pipeline is
/// offered to. `404` for an unknown pipeline, `400` naming an unknown Projet,
/// both before any write.
pub(super) async fn validate(
    State(pipelines): State<Arc<Pipelines>>,
    Extension(actor): Extension<Actor>,
    AxumPath(pipeline_id): AxumPath<String>,
    Json(validation): Json<PipelineValidation>,
) -> Response {
    match pipelines.validate(&actor, &pipeline_id, validation).await {
        Ok(stored) => Json(serde_json::json!({
            "ok": true,
            "id": pipeline_id,
            "validation": stored,
        }))
        .into_response(),
        Err(PipelineError::UnknownProject(project_id)) => error(
            StatusCode::BAD_REQUEST,
            format!("no such project: {project_id}"),
        ),
        Err(PipelineError::Storage(e)) => error(StatusCode::INTERNAL_SERVER_ERROR, e),
        Err(e) => fallback(e),
    }
}

pub(super) async fn delete(
    State(pipelines): State<Arc<Pipelines>>,
    Extension(actor): Extension<Actor>,
    AxumPath(pipeline_id): AxumPath<String>,
) -> Response {
    match pipelines.delete(&actor, &pipeline_id).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response(),
        Err(PipelineError::ActiveRuns(count)) => error(
            StatusCode::CONFLICT,
            format!("Cannot delete: {count} active run(s)"),
        ),
        Err(PipelineError::Storage(e)) => error(StatusCode::INTERNAL_SERVER_ERROR, e),
        Err(e) => fallback(e),
    }
}

/// A case no route of this file words: the shared answers, else a 500.
fn fallback(error: PipelineError) -> Response {
    common_response(&error)
        .unwrap_or_else(|| super::error(StatusCode::INTERNAL_SERVER_ERROR, format!("{error:?}")))
}

/// `201` with where the new pipeline landed, and the attributes the
/// extensions stamped on it (absent when none).
fn created_response(created: Created, stamped: Attributes) -> Response {
    let mut body = serde_json::json!({
        "id": created.id,
        "scope": "instance",
        "path": created.path.to_string_lossy(),
    });
    with_attributes(&mut body, stamped);
    (StatusCode::CREATED, Json(body)).into_response()
}

/// `body`, with the attributes of the entity it names when there are some.
pub(super) fn with_attributes(body: &mut serde_json::Value, attributes: Attributes) {
    if !attributes.is_empty() {
        body["attributes"] = serde_json::json!(attributes);
    }
}
