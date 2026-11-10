//! The Projet tier of a node's resolutions: the Projet the Run was launched in
//! (ADR-0087), read through the workspace context, and the settings it stores
//! for the downstream contexts (its harness, its auto-fail preference, its
//! agentic profile, its skills). A Run in no Projet has a transparent tier. A
//! store error is handed back, and every caller degrades the tier to
//! transparent.

use pdo_harness::agent_choice::AgentChoice;
use pdo_skill::domain::selection::{self as skill_selection, SkillRef};
use pdo_workspace::domain::project::Project;

use crate::Runs;

async fn project_of(runs: &Runs, project: Option<&str>) -> Result<Option<Project>, String> {
    match project {
        Some(id) => runs.projects.project(id).await,
        None => Ok(None),
    }
}

/// The `project` tier of the harness resolver. An empty stored harness is
/// already `None`.
pub async fn project_harness(runs: &Runs, project: Option<&str>) -> Result<Option<String>, String> {
    Ok(project_of(runs, project).await?.and_then(|p| p.harness))
}

/// The `project` tier of the auto-fail preference. `None` ⇒ the Run is in no
/// Projet, or it states no preference — transparent either way.
pub async fn project_auto_fail(runs: &Runs, project: Option<&str>) -> Result<Option<bool>, String> {
    Ok(project_of(runs, project).await?.and_then(|p| p.auto_fail))
}

/// The Projet tier of `agent_choice::resolve`. `None` ⇒ transparent — the
/// legacy `harness` signal then applies. An unreadable stored choice reads as
/// none rather than failing the whole read.
pub async fn project_agent_choice(
    runs: &Runs,
    project: Option<&str>,
) -> Result<Option<AgentChoice>, String> {
    Ok(project_of(runs, project).await?.and_then(|project| {
        project
            .agent_choice
            .as_ref()
            .and_then(|stored| serde_json::from_str(stored.as_str()).ok())
    }))
}

/// The Projet tier of `skill_selection::resolve`: the skills the Projet
/// selects, or empty for a Run in no Projet.
pub async fn project_skills(runs: &Runs, project: Option<&str>) -> Result<Vec<SkillRef>, String> {
    Ok(project_of(runs, project)
        .await?
        .map(|project| {
            skill_selection::from_stored_json(
                project.skills.as_ref().map(|s| s.as_str().to_string()),
            )
        })
        .unwrap_or_default())
}
