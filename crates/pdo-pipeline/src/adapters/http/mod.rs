//! The pipeline context's HTTP routes: `/pipelines*`, `/library*` and
//! `/nodes/parse`.
//!
//! The contract is the one the daemon has always spoken: same paths, same
//! bodies, same statuses. The actor of each request comes from the `identify`
//! middleware, which the composition root puts in front of every route.

mod document;
mod dto;
mod library;
mod pipelines;

use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};

use crate::adapters::registry::NO_REGISTRY;
use crate::application::{PipelineError, Pipelines};
use crate::domain::pipeline::ParseError;

pub use document::skills_sidecar_response;

/// The pipeline context's routes, with their state.
///
/// ROUTER TRAP: a NEW top-level path prefix also needs its entry in the vite
/// dev proxy whitelist (`frontend/vite.config.ts`).
pub fn router(pipelines: Arc<Pipelines>) -> Router {
    Router::new()
        .route("/pipelines", get(pipelines::list))
        .route("/pipelines/{pipeline_id}", get(pipelines::detail))
        .route(
            "/pipelines/{pipeline_id}/duplicate",
            post(pipelines::duplicate),
        )
        .route("/pipelines/{pipeline_id}/rename", put(pipelines::rename))
        .route("/pipelines/{pipeline_id}/document", get(document::export))
        // #673 / ADR-0062: the skills sidecar of the portable document — a zip of
        // `<pipeline>.skills/<id>/…` to unpack beside the YAML. The import takes it
        // back as a base64 field of the same request; own body limit so a sidecar
        // carrying a few reference files is not refused by the 2 MB default.
        .route(
            "/pipelines/{pipeline_id}/document/skills",
            get(document::skills),
        )
        .route(
            "/pipelines/import",
            post(document::import).route_layer(axum::extract::DefaultBodyLimit::max(
                pdo_skill::adapters::http::SKILL_UPLOAD_BODY_LIMIT,
            )),
        )
        .route(
            "/pipelines/{pipeline_id}",
            put(pipelines::save).delete(pipelines::delete),
        )
        .route("/pipelines", post(pipelines::create))
        .route("/library", get(library::list))
        .route("/library", post(library::save))
        // Parse a single node's YAML into a canvas-instantiable spec. Disk-free.
        .route("/nodes/parse", post(library::parse_node))
        .route("/library/{name}", delete(library::delete))
        .route("/library/{name}/instantiate", post(library::instantiate))
        // #155 — import a Claude Code workflow .js as a draft pipeline.
        .route("/library/import", post(library::import_workflow))
        .with_state(pipelines)
}

/// `404 pipeline not found`, as plain text.
fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "pipeline not found").into_response()
}

/// `500 cannot determine instance pipeline directory`, as plain text.
fn no_registry() -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, NO_REGISTRY).into_response()
}

/// `{"error": …}` with `status`.
fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(serde_json::json!({ "error": message.into() }))).into_response()
}

/// `{"error": …, "message": …}` with `status`: the editor shows `message`.
fn error_with_message(status: StatusCode, error: String, message: String) -> Response {
    (
        status,
        Json(serde_json::json!({ "error": error, "message": message })),
    )
        .into_response()
}

/// A parse failure as the editor reads it: the error, the message it shows,
/// and the line when the YAML itself is broken.
fn structured_parse_error(prefix: &str, e: &ParseError) -> Response {
    let (message, line) = parse_error_to_structured(e);
    let mut body = serde_json::json!({ "error": format!("{prefix}: {e}"), "message": message });
    if let Some(l) = line {
        body["line"] = serde_json::json!(l);
    }
    (StatusCode::BAD_REQUEST, Json(body)).into_response()
}

/// A parse failure as the editor shows it: its message, and its line when the
/// YAML itself is broken.
pub fn parse_error_to_structured(e: &ParseError) -> (String, Option<usize>) {
    match e {
        ParseError::InvalidYaml(yaml_err) => {
            let loc = yaml_err.location();
            let line = loc.map(|l| l.line());
            (format!("{yaml_err}"), line)
        }
        ParseError::MissingField(field) => (format!("missing required field: {field}"), None),
        ParseError::InvalidField(message) => (message.clone(), None),
        ParseError::UndeclaredWhenField {
            node_id,
            port,
            field,
        } => (
            format!("switch node '{node_id}' output '{port}': when-clause field '{field}' not found in upstream schema"),
            None,
        ),
    }
}

/// The answers every route shares: a refusal, a missing pipeline, a missing
/// registry. `None` for the cases each route words its own way.
fn common_response(error: &PipelineError) -> Option<Response> {
    match error {
        PipelineError::Denied(denied) => Some(denied.clone().into_response()),
        PipelineError::NotFound => Some(not_found()),
        PipelineError::NoRegistry => Some(no_registry()),
        PipelineError::Skills(skill) => Some(pdo_skill::adapters::http::skill_error_response(
            skill.clone(),
        )),
        _ => None,
    }
}
