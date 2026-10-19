//! The Projet tier of a node's resolutions: the Projet recorded on the Run
//! (ADR-0087), read through the workspace port.

use pdo_harness::agent_choice::AgentChoice;
use pdo_run::application::node::project_tiers::*;
use pdo_skill::domain::selection::{self as skill_selection, SkillRef};
use pdo_workspace::domain::project::{Project, StoredSetting};

use crate::fakes::*;

/// The Projet's agentic profile and skills are stored opaque by the
/// workspace; the run context decodes them for the harness and skill tiers.
#[tokio::test]
async fn the_projet_tiers_decode_what_the_workspace_stores() {
    let w = world();
    let runs = &w.runs;
    assert_eq!(project_agent_choice(runs, Some("p1")).await.unwrap(), None);
    assert!(project_skills(runs, Some("p1")).await.unwrap().is_empty());

    let choice = AgentChoice::Profile {
        profile_id: "reviewer".to_string(),
    };
    let mut project = Project::named("p1", "Alpha");
    project.members = vec!["/repos/front".into()];
    project.harness = Some("codex".into());
    project.auto_fail = Some(true);
    project.agent_choice = Some(StoredSetting(serde_json::to_string(&choice).unwrap()));
    project.skills = skill_selection::to_stored_json(&[SkillRef {
        id: "tdd".into(),
        name: "TDD".into(),
    }])
    .map(StoredSetting);
    w.projects.0.lock().unwrap().push(project);

    assert_eq!(
        project_agent_choice(runs, Some("p1")).await.unwrap(),
        Some(choice)
    );
    let selected = project_skills(runs, Some("p1")).await.unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].id, "tdd");
    assert_eq!(
        project_harness(runs, Some("p1")).await.unwrap(),
        Some("codex".into())
    );
    assert_eq!(
        project_auto_fail(runs, Some("p1")).await.unwrap(),
        Some(true)
    );
}

#[tokio::test]
async fn a_run_in_no_projet_has_a_transparent_tier() {
    let w = world();
    let mut project = Project::named("p1", "Alpha");
    project.harness = Some("codex".into());
    w.projects.0.lock().unwrap().push(project);
    *w.projects.1.lock().unwrap() = true;
    // No Projet: nothing is read, not even a broken store.
    assert_eq!(project_harness(&w.runs, None).await.unwrap(), None);
    assert!(project_skills(&w.runs, None).await.unwrap().is_empty());
}

#[tokio::test]
async fn an_unreadable_workspace_is_handed_back_for_the_caller_to_degrade() {
    let w = world();
    let runs = &w.runs;
    *w.projects.1.lock().unwrap() = true;
    assert!(project_harness(runs, Some("p1")).await.is_err());
}
