//! The Projet a new Run is launched in (ADR-0087): its parent Run's for a Run
//! created from a node session, else the one the request names, else — for a
//! gesture — the one Projet holding its repository ([`Runs::place_new_run`]),
//! else none. Recorded in `RunStarted` and never recomputed: changing the
//! members of a Projet moves no Run.

use pdo_kernel::authorization::authorize_read;
use pdo_kernel::repository_path::member_spellings;
use pdo_kernel::Actor;
use pdo_workspace::application::read_access::read_project;
use pdo_workspace::domain::project::Project;
use tracing::{error, warn};

use super::request::CreateRunRequest;
use super::target::{bad_request, Refusal};
use crate::application::session_claim::SessionClaim;
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

/// The Projet of the new Run: the one of `parent`, the Run whose node
/// session creates it (ADR-0064) — a child stays in its parent's Projet, and
/// naming another is refused — else the one `req` names, else none.
///
/// A named Projet that no longer exists is no Projet: a Trigger or a retried
/// Run may name one deleted since, and its Run launches in none, as the Run
/// did before (a gesture naming an unknown Projet was refused before, by
/// [`Runs::place_new_run`]).
pub(super) async fn resolve(
    state: &Runs,
    req: &CreateRunRequest,
    parent: Option<&RunState>,
) -> Result<Option<String>, Refusal> {
    if let Some(parent) = parent {
        return match named_project(req) {
            Some(id) if parent.project_id.as_deref() != Some(id) => Err(bad_request(format!(
                "a run created from a node session stays in its parent's project, not `{id}`"
            ))),
            _ => Ok(parent.project_id.clone()),
        };
    }
    match named_project(req) {
        Some(id) => match state.projects.project(id).await {
            Ok(Some(project)) => Ok(Some(project.id)),
            Ok(None) => {
                warn!("project `{id}` no longer exists: the run is launched in no project");
                Ok(None)
            }
            Err(e) => Err(store_unavailable(e)),
        },
        None => Ok(None),
    }
}

/// The primary repository a request names: `target_repo`, else
/// `target_repos[0]`, trimmed, when not blank.
fn named_primary(req: &CreateRunRequest) -> Option<&str> {
    req.target_repo
        .as_deref()
        .or_else(|| req.target_repos.first().map(|r| r.repo.as_str()))
        .map(str::trim)
        .filter(|repo| !repo.is_empty())
}

/// The secondary repositories a request names: `target_repos[1..]`,
/// trimmed, not blank.
fn named_secondaries(req: &CreateRunRequest) -> impl Iterator<Item = &str> {
    req.target_repos
        .iter()
        .skip(1)
        .map(|r| r.repo.trim())
        .filter(|repo| !repo.is_empty())
}

/// Whether `project` holds `repo`, under any of its spellings (`/repo/` and
/// `/repo/.` stand for `/repo`).
fn holds(project: &Project, repo: &str) -> bool {
    member_spellings(repo)
        .iter()
        .any(|spelling| project.has_member(spelling))
}

/// « Alpha », « Alpha and Beta », « Alpha, Beta and Gamma ».
fn and_list(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// The sentence of a repository held by several Projets, a launch naming
/// none: it names the Projets, the CLI and the New Run form say the same.
pub fn ambiguous_repository(names: &[String]) -> String {
    format!(
        "This repository is in {}: pick its Project.",
        and_list(names)
    )
}

impl Runs {
    /// Where the Run a gesture of `actor` creates is launched (ADR-0087): the
    /// Projets of the `run` × `create` it asks, decided before it is asked.
    ///
    /// - From a node session (ADR-0064), its parent Run's Projet.
    /// - Naming a Projet, that Projet.
    /// - Naming none, the Projet holding the primary repository, written in
    ///   `req` so the Run records it: when several do, the launch is refused,
    ///   naming them (those `actor` may read; the others counted), and it
    ///   must name one; when none does, no Projet.
    ///
    /// Its repositories are checked once the creation is allowed
    /// ([`Self::check_new_run_repositories`]).
    pub(super) async fn place_new_run(
        &self,
        actor: &Actor,
        req: &mut CreateRunRequest,
        session: Option<&SessionClaim>,
    ) -> Result<Vec<String>, Refusal> {
        if let Some(claim) = session {
            return Ok(self.projects_of_run(&claim.run_id).await);
        }
        if let Some(id) = named_project(req) {
            return Ok(vec![id.to_string()]);
        }
        let Some(primary) = named_primary(req).map(str::to_string) else {
            return Ok(Vec::new());
        };
        let holding = self
            .projects
            .holding(&primary)
            .await
            .map_err(store_unavailable)?;
        match holding.as_slice() {
            [] => Ok(Vec::new()),
            [project] => {
                req.project_id = Some(project.id.clone());
                Ok(vec![project.id.clone()])
            }
            several => Err(self.ambiguous(actor, several)),
        }
    }

    /// The repositories of a new Run launched in `projects`
    /// ([`Self::place_new_run`]), once its creation is allowed: the primary
    /// one is a member of its Projet — from a node session whose parent is in
    /// no Projet, of no Projet. Answers whether the Run reads the disk, which
    /// the caller asks `repository` × `read` in no Projet about: a Run in no
    /// Projet does, and so does one naming a secondary repository outside its
    /// Projet.
    pub(super) async fn check_new_run_repositories(
        &self,
        req: &CreateRunRequest,
        from_session: bool,
        projects: &[String],
    ) -> Result<bool, Refusal> {
        let primary = named_primary(req);
        let Some(id) = projects.first() else {
            if let (true, Some(primary)) = (from_session, primary) {
                self.check_child_outside_projects(primary).await?;
            }
            return Ok(true);
        };
        let project = match self.projects.project(id).await {
            Ok(Some(project)) => project,
            // A child's parent Projet deleted since holds nothing to check.
            Ok(None) if from_session => return Ok(true),
            Ok(None) => return Err(bad_request(format!("unknown project `{id}`"))),
            Err(e) => return Err(store_unavailable(e)),
        };
        if let Some(primary) = primary {
            if !holds(&project, primary) {
                return Err(bad_request(format!(
                    "repository `{primary}` is not a member of project `{}`",
                    project.name
                )));
            }
        }
        Ok(named_secondaries(req).any(|repo| !holds(&project, repo)))
    }

    /// A Run created from a node session of a Run in no Projet works in a
    /// repository of no Projet.
    async fn check_child_outside_projects(&self, primary: &str) -> Result<(), Refusal> {
        let holding = self
            .projects
            .holding(primary)
            .await
            .map_err(store_unavailable)?;
        if holding.is_empty() {
            return Ok(());
        }
        Err(bad_request(format!(
            "a run created from a node session of a run in no project works in a repository \
             of no project, not `{primary}`"
        )))
    }

    /// A repository held by `several` Projets, named to `actor`: the Projets
    /// it may read by name, the others counted.
    fn ambiguous(&self, actor: &Actor, several: &[Project]) -> Refusal {
        let (readable, hidden): (Vec<&Project>, Vec<&Project>) =
            several.iter().partition(|project| {
                authorize_read(self.extensions(), actor, &read_project(&project.id)).is_ok()
            });
        let mut names: Vec<String> = readable.iter().map(|p| p.name.clone()).collect();
        match hidden.len() {
            0 => {}
            1 => names.push("another Project".to_string()),
            n => names.push(format!("{n} other Projects")),
        }
        let projects: Vec<serde_json::Value> = readable
            .iter()
            .map(|p| serde_json::json!({ "id": p.id, "name": p.name }))
            .collect();
        (
            StatusCode::BAD_REQUEST,
            serde_json::json!({
                "error": ambiguous_repository(&names),
                "projects": projects,
            }),
        )
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
        if holds(&project, primary) {
            Ok(())
        } else {
            Err(bad_request(format!(
                "repository `{primary}` is not a member of project `{}`",
                project.name
            )))
        }
    }
}
