//! `/pages*` — list, mount, unmount, and serve the mounted pages.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;
use serde::{Deserialize, Serialize};

use super::{error, OriginHint};
use crate::application::mount_page::{MountPageCommand, MountPageError};
use crate::application::unmount_page::UnmountPageError;
use crate::domain::page_mount::{MountError, PageMount};
use crate::Platform;

const STORAGE_ERROR: &str = "page mount storage error";
const EVENT_LOG_ERROR: &str = "page mount event log error";

#[derive(Deserialize)]
pub(super) struct CreatePageMountRequest {
    name: String,
    directory: PathBuf,
    /// The guide file's text (YAML or JSON), checked before the mount.
    #[serde(default)]
    guide: Option<String>,
}

/// The mount, and the instance's Public URL its links are printed under
/// (`null` when unset: the caller's own daemon address).
#[derive(Serialize)]
struct MountedPage {
    #[serde(flatten)]
    mount: PageMount,
    public_url: Option<String>,
}

pub(super) async fn list(State(platform): State<Arc<Platform>>) -> Response {
    match platform.read_pages.list().await {
        Ok(mounts) => Json(mounts).into_response(),
        Err(e) => {
            tracing::error!("failed to list page mounts: {e}");
            error(StatusCode::INTERNAL_SERVER_ERROR, STORAGE_ERROR)
        }
    }
}

pub(super) async fn mount(
    State(platform): State<Arc<Platform>>,
    Extension(actor): Extension<Actor>,
    OriginHint(hint): OriginHint,
    Json(request): Json<CreatePageMountRequest>,
) -> Response {
    let command = MountPageCommand {
        name: request.name,
        directory: request.directory,
        guide: request.guide,
        hint,
    };
    match platform.mount_page.execute(&actor, command).await {
        Ok(mount) => {
            let public_url = platform.public_url().await;
            (StatusCode::CREATED, Json(MountedPage { mount, public_url })).into_response()
        }
        Err(MountPageError::Refused(denied)) => denied.into_response(),
        Err(MountPageError::SessionRefused(message)) => error(StatusCode::FORBIDDEN, message),
        Err(MountPageError::Invalid(MountError::Invalid(message))) => {
            error(StatusCode::BAD_REQUEST, message)
        }
        Err(MountPageError::Invalid(duplicate @ MountError::Duplicate(_))) => {
            error(StatusCode::CONFLICT, duplicate.to_string())
        }
        Err(MountPageError::Storage(e)) => {
            tracing::error!("failed to create page mount: {e}");
            error(StatusCode::INTERNAL_SERVER_ERROR, STORAGE_ERROR)
        }
        Err(MountPageError::Journal(e)) => {
            tracing::error!("failed to record page mount event: {e}");
            error(StatusCode::INTERNAL_SERVER_ERROR, EVENT_LOG_ERROR)
        }
    }
}

pub(super) async fn unmount(
    State(platform): State<Arc<Platform>>,
    Extension(actor): Extension<Actor>,
    OriginHint(hint): OriginHint,
    AxumPath(name): AxumPath<String>,
) -> Response {
    match platform.unmount_page.execute(&actor, &name, hint).await {
        Ok(mount) => Json(mount).into_response(),
        Err(UnmountPageError::Refused(denied)) => denied.into_response(),
        Err(UnmountPageError::SessionRefused(message)) => error(StatusCode::FORBIDDEN, message),
        Err(UnmountPageError::NotFound(name)) => error(
            StatusCode::NOT_FOUND,
            format!("page mount `{name}` not found"),
        ),
        Err(UnmountPageError::Storage(e)) => {
            tracing::error!("failed to remove page mount: {e}");
            error(StatusCode::INTERNAL_SERVER_ERROR, STORAGE_ERROR)
        }
        Err(UnmountPageError::Journal(e)) => {
            tracing::error!("failed to record page unmount event: {e}");
            error(StatusCode::INTERNAL_SERVER_ERROR, EVENT_LOG_ERROR)
        }
    }
}

/// `/pages/{name}` → `/pages/{name}/`, so the page's relative links resolve.
pub(super) async fn redirect(
    State(platform): State<Arc<Platform>>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    match platform.read_pages.get(&name).await {
        Ok(Some(_)) => Redirect::permanent(&format!("/pages/{name}/")).into_response(),
        Ok(None) => page_not_found(),
        Err(e) => {
            tracing::error!("failed to resolve page mount: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub(super) async fn serve_root(
    State(platform): State<Arc<Platform>>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    serve(&platform, &name, "").await
}

pub(super) async fn serve_file(
    State(platform): State<Arc<Platform>>,
    AxumPath((name, path)): AxumPath<(String, String)>,
) -> Response {
    serve(&platform, &name, &path).await
}

async fn serve(platform: &Platform, name: &str, path: &str) -> Response {
    match platform.read_pages.file(name, path).await {
        Ok(Some(file)) => {
            let mime = mime_guess::from_path(&file.mime_path).first_or_octet_stream();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime.as_ref())],
                file.contents,
            )
                .into_response()
        }
        Ok(None) => page_not_found(),
        Err(e) => {
            tracing::error!("failed to serve mounted page: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

fn page_not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        "page not found",
    )
        .into_response()
}
