//! The Projet a new Run is launched in (ADR-0087): the one the request names,
//! else its parent Run's, else none. Recorded in `RunStarted` and never
//! recomputed: changing the members of a Projet moves no Run.

use pdo_kernel::repository_path::member_spellings;
use tracing::error;

use super::request::CreateRunRequest;
use super::target::{bad_request, Refusal};
use crate::domain::state::RunState;
use crate::Runs;
use axum::http::StatusCode;

/// The Projet a request names: a field that is not blank, trimmed.
pub(crate) fn named_project(req: &CreateRunRequest) -> Option<&str> {
    req.project_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
}

fn store_unavailable(e: String) -> Refusal {
    error!("the Projets cannot be read: {e}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        serde_json::json!({ "error": "project store unavailable" }),
    )
}

/// The Projet of the new Run: the one `req` names (it must exist), else the
/// one of `parent`, the Run whose node session creates it (ADR-0064), else
/// none.
pub(super) async fn resolve(
    state: &Runs,
    req: &CreateRunRequest,
    parent: Option<&RunState>,
) -> Result<Option<String>, Refusal> {
    match named_project(req) {
        Some(id) => match state.projects.project(id).await {
            Ok(Some(project)) => Ok(Some(project.id)),
            Ok(None) => Err(bad_request(format!("unknown project `{id}`"))),
            Err(e) => Err(store_unavailable(e)),
        },
        None => Ok(parent.and_then(|run| run.project_id.clone())),
    }
}

impl Runs {
    /// A launch that names a Projet works in one of its repositories: the
    /// primary repository the request names must be a member (`/repo/` and
    /// `/repo/.` stand for `/repo`). A request naming no repository, or no
    /// Projet, is not checked here.
    pub(super) async fn check_primary_in_project(
        &self,
        req: &CreateRunRequest,
    ) -> Result<(), Refusal> {
        let Some(id) = named_project(req) else {
            return Ok(());
        };
        let primary = req
            .target_repo
            .as_deref()
            .or_else(|| req.target_repos.first().map(|r| r.repo.as_str()));
        self.check_launch_in_project(id, primary).await
    }

    /// Whether a Run may be launched in the Projet `id` on the primary
    /// repository `primary` (ADR-0087): the Projet exists and, when a
    /// repository is named, holds it (`/repo/` and `/repo/.` stand for
    /// `/repo`). A Trigger, a Run template, is checked the same way.
    pub async fn check_launch_in_project(
        &self,
        id: &str,
        primary: Option<&str>,
    ) -> Result<(), Refusal> {
        let project = match self.projects.project(id).await {
            Ok(Some(project)) => project,
            Ok(None) => return Err(bad_request(format!("unknown project `{id}`"))),
            Err(e) => return Err(store_unavailable(e)),
        };
        let Some(primary) = primary.map(str::trim).filter(|repo| !repo.is_empty()) else {
            return Ok(());
        };
        if member_spellings(primary)
            .iter()
            .any(|spelling| project.has_member(spelling))
        {
            Ok(())
        } else {
            Err(bad_request(format!(
                "repository `{primary}` is not a member of project `{}`",
                project.name
            )))
        }
    }
}
