//! `/projects*`: a Projet is the middle tier of the harness axis and a named
//! grouping of member repo paths. Nothing is seeded: a row exists only once a
//! human names a group or attaches a setting (ADR-0046).

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;
use tracing::error;

use crate::application::{MemberAdded, MemberRemoved, WorkspaceError};
use crate::domain::project::Project;

use super::dto::{CreateProjectRequest, PatchProjectRequest, ProjectDto, ProjectMemberRequest};
use super::{error as error_body, Http, ProjectSettings};

fn store_error() -> Response {
    error_body(
        StatusCode::INTERNAL_SERVER_ERROR,
        "project store unavailable",
    )
}

fn no_such_project() -> Response {
    (StatusCode::NOT_FOUND, "no such project").into_response()
}

fn project_json<S: ProjectSettings>(project: Project) -> Response {
    Json(ProjectDto::from_project::<S>(project)).into_response()
}

/// The response of a use case that failed, in the project routes' vocabulary.
fn failure(err: WorkspaceError) -> Response {
    match err {
        WorkspaceError::Denied(d) => d.into_response(),
        WorkspaceError::Invalid(message) => error_body(StatusCode::BAD_REQUEST, message),
        WorkspaceError::NotFound => no_such_project(),
        WorkspaceError::Storage(e) => {
            error!("{e}");
            store_error()
        }
        WorkspaceError::Git(e) => {
            error!("{e}");
            store_error()
        }
    }
}

pub(super) async fn list<S: ProjectSettings>(State(http): State<Http<S>>) -> Response {
    match http.workspace.list_projects().await {
        Ok(projects) => Json(
            projects
                .into_iter()
                .map(ProjectDto::from_project::<S>)
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(e) => {
            error!("failed to list projects: {e}");
            store_error()
        }
    }
}

pub(super) async fn get<S: ProjectSettings>(
    State(http): State<Http<S>>,
    AxumPath(project_id): AxumPath<String>,
) -> Response {
    match http.workspace.project(&project_id).await {
        Ok(Some(project)) => project_json::<S>(project),
        Ok(None) => no_such_project(),
        Err(e) => {
            error!("failed to load project {project_id}: {e}");
            store_error()
        }
    }
}

pub(super) async fn create<S: ProjectSettings>(
    State(http): State<Http<S>>,
    Extension(actor): Extension<Actor>,
    Json(req): Json<CreateProjectRequest>,
) -> Response {
    match http.workspace.create_project(&actor, &req.name).await {
        Ok(project) => (
            StatusCode::CREATED,
            Json(ProjectDto::from_project::<S>(project)),
        )
            .into_response(),
        Err(e) => failure(e),
    }
}

pub(super) async fn patch<S: ProjectSettings>(
    State(http): State<Http<S>>,
    Extension(actor): Extension<Actor>,
    AxumPath(project_id): AxumPath<String>,
    Json(req): Json<PatchProjectRequest<S::AgentChoice, S::Skill>>,
) -> Response {
    let update = req.into_update::<S>();
    match http
        .workspace
        .update_project(&actor, &project_id, update)
        .await
    {
        Ok(project) => project_json::<S>(project),
        Err(e) => failure(e),
    }
}

pub(super) async fn delete<S: ProjectSettings>(
    State(http): State<Http<S>>,
    Extension(actor): Extension<Actor>,
    AxumPath(project_id): AxumPath<String>,
) -> Response {
    match http.workspace.delete_project(&actor, &project_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => failure(e),
    }
}

pub(super) async fn add_member<S: ProjectSettings>(
    State(http): State<Http<S>>,
    Extension(actor): Extension<Actor>,
    AxumPath(project_id): AxumPath<String>,
    Json(req): Json<ProjectMemberRequest>,
) -> Response {
    match http
        .workspace
        .add_project_member(&actor, &project_id, &req.path)
        .await
    {
        Ok(MemberAdded::Attached(Some(project))) => project_json::<S>(project),
        Ok(MemberAdded::Attached(None)) => StatusCode::NO_CONTENT.into_response(),
        // A path already owned by another Projet is refused BEFORE any effect, and
        // the refusal NAMES the owner so the pencil can say which one.
        Ok(MemberAdded::Refused {
            owner_id,
            owner_name,
        }) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": format!("path already belongs to project '{owner_name}'"),
                "owner_id": owner_id,
                "owner_name": owner_name,
            })),
        )
            .into_response(),
        Err(e) => failure(e),
    }
}

pub(super) async fn remove_member<S: ProjectSettings>(
    State(http): State<Http<S>>,
    Extension(actor): Extension<Actor>,
    AxumPath(project_id): AxumPath<String>,
    Json(req): Json<ProjectMemberRequest>,
) -> Response {
    match http
        .workspace
        .remove_project_member(&actor, &project_id, &req.path)
        .await
    {
        Ok(MemberRemoved::Untouched(project)) | Ok(MemberRemoved::Detached(Some(project))) => {
            project_json::<S>(project)
        }
        Ok(MemberRemoved::Detached(None)) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => failure(e),
    }
}
