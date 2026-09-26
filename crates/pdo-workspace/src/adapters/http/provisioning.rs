//! Provisioning rules over HTTP: the instance's, a Projet's, and the preview the
//! editors resolve against the source repository.

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;
use serde::Deserialize;
use tracing::error;

use crate::application::{PreviewRequest, RulesOwner, WorkspaceError, INSTANCE_SCOPE_ID};
use crate::domain::provisioning::{ProvisioningRules, ProvisioningScope, ScopedRules};

use super::{denied, error as error_body, Http, ProjectSettings};

#[derive(Deserialize)]
pub(super) struct ProvisioningPreviewRequest {
    repository: String,
    #[serde(default = "default_git_ref")]
    git_ref: String,
    scope: ProvisioningScope,
    #[serde(default)]
    rules: ProvisioningRules,
    #[serde(default)]
    inherited: Option<Vec<ScopedRules>>,
}

fn default_git_ref() -> String {
    "HEAD".to_string()
}

fn store_error() -> Response {
    error_body(
        StatusCode::INTERNAL_SERVER_ERROR,
        "provisioning store unavailable",
    )
}

fn rules_response(result: Result<ProvisioningRules, impl std::fmt::Display>) -> Response {
    match result {
        Ok(rules) => (StatusCode::OK, Json(rules)).into_response(),
        Err(e) => {
            error!("failed to load provisioning rules: {e}");
            store_error()
        }
    }
}

fn saved(result: Result<ProvisioningRules, WorkspaceError>) -> Response {
    match result {
        Ok(rules) => (StatusCode::OK, Json(rules)).into_response(),
        Err(WorkspaceError::Denied(d)) => denied(d),
        Err(WorkspaceError::NotFound) => error_body(StatusCode::NOT_FOUND, "project not found"),
        Err(WorkspaceError::Invalid(message)) => error_body(StatusCode::BAD_REQUEST, message),
        Err(WorkspaceError::Storage(e)) => {
            error!("{e}");
            store_error()
        }
        Err(WorkspaceError::Git(e)) => {
            error!("{e}");
            store_error()
        }
    }
}

pub(super) async fn preview<S: ProjectSettings>(
    State(http): State<Http<S>>,
    Json(req): Json<ProvisioningPreviewRequest>,
) -> Response {
    let request = PreviewRequest {
        repository: req.repository,
        git_ref: req.git_ref,
        scope: req.scope,
        rules: req.rules,
        inherited: req.inherited,
    };
    match http.workspace.preview_provisioning(request).await {
        Ok(plan) => (StatusCode::OK, Json(plan)).into_response(),
        Err(WorkspaceError::Storage(e)) => {
            error!("{e}");
            store_error()
        }
        Err(WorkspaceError::Denied(d)) => denied(d),
        Err(WorkspaceError::Invalid(message)) | Err(WorkspaceError::Git(message)) => {
            error_body(StatusCode::BAD_REQUEST, message)
        }
        Err(WorkspaceError::NotFound) => StatusCode::NOT_FOUND.into_response(),
    }
}

pub(super) async fn get_instance<S: ProjectSettings>(State(http): State<Http<S>>) -> Response {
    rules_response(
        http.workspace
            .provisioning_rules(ProvisioningScope::Instance, INSTANCE_SCOPE_ID)
            .await,
    )
}

pub(super) async fn put_instance<S: ProjectSettings>(
    State(http): State<Http<S>>,
    Extension(actor): Extension<Actor>,
    Json(rules): Json<ProvisioningRules>,
) -> Response {
    saved(
        http.workspace
            .save_provisioning_rules(&actor, RulesOwner::Instance, rules)
            .await,
    )
}

pub(super) async fn get_project<S: ProjectSettings>(
    State(http): State<Http<S>>,
    AxumPath(project_id): AxumPath<String>,
) -> Response {
    rules_response(
        http.workspace
            .provisioning_rules(ProvisioningScope::Project, &project_id)
            .await,
    )
}

pub(super) async fn put_project<S: ProjectSettings>(
    State(http): State<Http<S>>,
    Extension(actor): Extension<Actor>,
    AxumPath(project_id): AxumPath<String>,
    Json(rules): Json<ProvisioningRules>,
) -> Response {
    saved(
        http.workspace
            .save_provisioning_rules(&actor, RulesOwner::Project(project_id), rules)
            .await,
    )
}
