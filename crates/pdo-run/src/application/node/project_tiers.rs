//! The Projet tier of a node's resolutions: the Projet that owns a Run's
//! primary repository, read through the workspace context, and the settings it
//! stores for the downstream contexts (its harness, its auto-fail preference,
//! its agentic profile, its skills). A store error is handed back, and every
//! caller degrades the tier to transparent.

use pdo_harness::agent_choice::AgentChoice;
use pdo_skill::domain::selection::{self as skill_selection, SkillRef};
use pdo_workspace::domain::project::Project;

use crate::Runs;

async fn owner_of(runs: &Runs, path: &str) -> Result<Option<Project>, String> {
    runs.projects.owner_of(path).await
}

/// The `project` tier of the harness resolver. An empty stored harness is
/// already `None`.
pub async fn project_harness_for_path(runs: &Runs, path: &str) -> Result<Option<String>, String> {
    Ok(owner_of(runs, path).await?.and_then(|p| p.harness))
}

/// The `project` tier of the auto-fail preference. `None` ⇒ no Projet owns the
/// path, or it states no preference — transparent either way.
pub async fn project_auto_fail_for_path(runs: &Runs, path: &str) -> Result<Option<bool>, String> {
    Ok(owner_of(runs, path).await?.and_then(|p| p.auto_fail))
}

/// The Projet tier of `agent_choice::resolve`. `None` ⇒ transparent — the
/// legacy `harness` signal then applies. An unreadable stored choice reads as
/// none rather than failing the whole read.
pub async fn project_agent_choice_for_path(
    runs: &Runs,
    path: &str,
) -> Result<Option<AgentChoice>, String> {
    Ok(owner_of(runs, path).await?.and_then(|project| {
        project
            .agent_choice
            .as_ref()
            .and_then(|stored| serde_json::from_str(stored.as_str()).ok())
    }))
}

/// The Projet tier of `skill_selection::resolve`: the skills selected by the
/// Projet owning `path`, or empty when no Projet owns it.
pub async fn project_skills_for_path(runs: &Runs, path: &str) -> Result<Vec<SkillRef>, String> {
    Ok(owner_of(runs, path)
        .await?
        .map(|project| {
            skill_selection::from_stored_json(
                project.skills.as_ref().map(|s| s.as_str().to_string()),
            )
        })
        .unwrap_or_default())
}
