//! `/repos*`: validate a repository, list and refresh its branches,
//! fast-forward one, read its remote, create a repository, and the recent ones.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;
use serde::Deserialize;
use tracing::error;

use crate::application::{FastForwarded, WorkspaceError};
use crate::domain::scaffold::ScaffoldFile;

use super::dto::RepoRemote;
use super::{error as error_body, Http, ProjectSettings};

#[derive(Deserialize)]
pub(super) struct RepoPathQuery {
    path: String,
}

#[derive(Deserialize)]
pub(super) struct FastForwardBody {
    /// The LOCAL branch to advance. A remote-tracking ref is refused up front — it
    /// is the upstream, there is nothing to move.
    branch: String,
}

#[derive(Deserialize)]
pub(super) struct CreateRepoRequest {
    parent: String,
    name: String,
    #[serde(default)]
    files: Vec<ScaffoldFile>,
}

/// A failure of the repository routes: a path that is not a repository is a
/// 400, a list git cannot read a 500, both with the message.
fn failure(err: WorkspaceError) -> Response {
    match err {
        WorkspaceError::Denied(d) => d.into_response(),
        WorkspaceError::Invalid(message) => error_body(StatusCode::BAD_REQUEST, message),
        WorkspaceError::Git(message) => error_body(StatusCode::INTERNAL_SERVER_ERROR, message),
        WorkspaceError::Storage(e) => error_body(StatusCode::INTERNAL_SERVER_ERROR, e.0),
        WorkspaceError::NotFound => StatusCode::NOT_FOUND.into_response(),
    }
}

pub(super) async fn branches<S: ProjectSettings>(
    State(http): State<Http<S>>,
    Query(q): Query<RepoPathQuery>,
) -> Response {
    match http.workspace.branches(&q.path) {
        Ok(list) => Json(list).into_response(),
        Err(e) => failure(e),
    }
}

/// Fetch every remote of the target repo, then answer the refreshed list (#802).
///
/// A failed fetch is **200 with `fetch_error` set**, not an HTTP error: the list is
/// still the truth on disk, launching is still possible, and a 4xx/5xx would push the
/// client into an error path for a repo that is merely offline (ADR-0070 §1).
pub(super) async fn fetch<S: ProjectSettings>(
    State(http): State<Http<S>>,
    Extension(actor): Extension<Actor>,
    Query(q): Query<RepoPathQuery>,
) -> Response {
    match http.workspace.fetch_remotes(&actor, &q.path).await {
        Ok(list) => Json(list).into_response(),
        Err(e) => failure(e),
    }
}

/// Fast-forward a local branch onto its tracking branch (#803, ADR-0070 §2).
///
/// **200** with what moved plus the refreshed list; **409** with the named refusal
/// AND the same refreshed list, so the two benign races (`up_to_date`, `no_upstream`)
/// let the client flip to the corresponding #802 state instead of painting an error.
/// A 400 is reserved for a request that never made sense: a repo that is not a repo,
/// an empty branch, a branch that is not a local one.
pub(super) async fn fast_forward<S: ProjectSettings>(
    State(http): State<Http<S>>,
    Extension(actor): Extension<Actor>,
    Query(q): Query<RepoPathQuery>,
    Json(body): Json<FastForwardBody>,
) -> Response {
    match http
        .workspace
        .fast_forward_branch(&actor, &q.path, &body.branch)
        .await
    {
        Ok(FastForwarded::Done(ok)) => Json(ok).into_response(),
        Ok(FastForwarded::Refused(refused)) => {
            (StatusCode::CONFLICT, Json(refused)).into_response()
        }
        Err(e) => failure(e),
    }
}

/// `GET /repos/remote?path=`: `{"remote": url}` for the repository at `path`,
/// its credentials dropped; `null` for a repository without a remote.
pub(super) async fn remote<S: ProjectSettings>(
    State(http): State<Http<S>>,
    Query(q): Query<RepoPathQuery>,
) -> Response {
    match http.workspace.repository_remote(&q.path) {
        Ok(remote) => Json(RepoRemote { remote }).into_response(),
        Err(e) => failure(e),
    }
}

pub(super) async fn validate<S: ProjectSettings>(
    State(http): State<Http<S>>,
    Query(q): Query<RepoPathQuery>,
) -> Response {
    match http.workspace.validate_repository(&q.path) {
        Ok(_) => Json(serde_json::json!({ "valid": true })).into_response(),
        Err(msg) => Json(serde_json::json!({ "valid": false, "error": msg })).into_response(),
    }
}

/// POST /repos/create — make a git repository at `<parent>/<name>` (#824).
///
/// A generic verb with no tour vocabulary in it (ADR-0071 §3): the *First run*
/// tour calls it with `/tmp` + `pdo-tutorial`, and that is the only thing tying
/// the two together. Idempotent on an existing repository, refuses by name on an
/// existing non-repository.
pub(super) async fn create<S: ProjectSettings>(
    State(http): State<Http<S>>,
    Extension(actor): Extension<Actor>,
    Json(req): Json<CreateRepoRequest>,
) -> Response {
    match http
        .workspace
        .create_repository(&actor, &req.parent, &req.name, &req.files)
        .await
    {
        Ok(outcome) => Json(serde_json::json!({
            "path": outcome.path.to_string_lossy(),
            "created": outcome.created,
        }))
        .into_response(),
        Err(e) => failure(e),
    }
}

/// `GET /repos/recent`: only the repositories of Runs the caller may read
/// (CORE-5).
pub(super) async fn recent<S: ProjectSettings>(
    State(http): State<Http<S>>,
    Extension(actor): Extension<Actor>,
) -> Response {
    match http.workspace.recent_repositories(&actor).await {
        Ok(repos) => Json(repos).into_response(),
        Err(e) => {
            error!("failed to query recent repos: {e}");
            error_body(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to query recent repos",
            )
        }
    }
}
