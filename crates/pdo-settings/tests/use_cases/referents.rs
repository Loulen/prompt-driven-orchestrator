use pdo_harness::agent_choice::AgentChoice;
use pdo_settings::application::SettingsError;
use pdo_settings::domain::agent_profile::DEFAULT_PROFILE_ID;
use pdo_settings::domain::referents::{PipelineReferent, ProjectReferent, TriggerReferent};

use crate::fakes::{world, world_with, EchoCap, Referents};

#[tokio::test]
async fn the_referents_list_every_tier_naming_the_profile() {
    let w = world_with(
        vec![std::sync::Arc::new(EchoCap)],
        Referents {
            projects: vec![ProjectReferent {
                id: "prj-1".into(),
                name: "App".into(),
            }],
            triggers: vec![TriggerReferent {
                id: "trg-1".into(),
                name: "Nightly".into(),
                pipeline_id: "nightly".into(),
            }],
            pipelines: vec![PipelineReferent {
                id: "flow".into(),
                name: "Flow".into(),
                node_id: "review".into(),
                scope: "instance".into(),
            }],
        },
    );
    w.config.row.lock().unwrap().agent_choice = Some(AgentChoice::Profile {
        profile_id: DEFAULT_PROFILE_ID.into(),
    });
    let referents = w
        .settings
        .agent_profile_referents(DEFAULT_PROFILE_ID)
        .await
        .unwrap();
    assert!(referents.instance, "the instance tier names it");
    assert_eq!(referents.projects.len(), 1);
    assert_eq!(referents.triggers[0].pipeline_id, "nightly");
    assert_eq!(referents.pipelines[0].node_id, "review");
    assert!(referents.runs.is_empty(), "a Run froze its combination");
    assert!(w.asked().is_empty(), "a read asks no one");
}

#[tokio::test]
async fn the_instance_names_only_the_profile_it_selects() {
    let w = world();
    w.config.row.lock().unwrap().agent_choice = Some(AgentChoice::Profile {
        profile_id: "agp-other".into(),
    });
    let referents = w
        .settings
        .agent_profile_referents(DEFAULT_PROFILE_ID)
        .await
        .unwrap();
    assert!(!referents.instance);
}

#[tokio::test]
async fn an_unknown_profile_has_no_referents_but_a_404() {
    let w = world();
    assert!(matches!(
        w.settings
            .agent_profile_referents("agp-nope")
            .await
            .unwrap_err(),
        SettingsError::NotFound(_)
    ));
}
