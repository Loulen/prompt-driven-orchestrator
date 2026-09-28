use std::sync::Arc;

use pdo_harness::agent_choice::AgentChoice;
use pdo_settings::application::SettingsError;
use pdo_skill::domain::selection::SkillRef;
use serde_json::{json, Map};

use crate::fakes::{world, world_with, FailingSection, FixedSection, Referents};

#[tokio::test]
async fn the_page_is_every_section_plus_the_instance_tiers_the_context_owns() {
    let mut run = Map::new();
    run.insert("session_cap".into(), json!({ "effective": 20 }));
    let mut sandbox = Map::new();
    sandbox.insert("default_sandbox".into(), json!({ "effective": "off" }));
    let w = world_with(
        vec![Arc::new(FixedSection(run)), Arc::new(FixedSection(sandbox))],
        Referents::default(),
    );
    {
        let mut row = w.config.row.lock().unwrap();
        row.agent_choice = Some(AgentChoice::Profile {
            profile_id: "agp-1".into(),
        });
        row.skills = vec![SkillRef {
            id: "tdd".into(),
            name: "TDD".into(),
        }];
    }
    let page = w.settings.view().await.unwrap();
    assert_eq!(
        page,
        json!({
            "session_cap": { "effective": 20 },
            "default_sandbox": { "effective": "off" },
            "agent_choice": { "mode": "profile", "profile_id": "agp-1" },
            "skills": [{ "id": "tdd", "name": "TDD" }],
            "updated_at": "2026-01-01T00:00:00.000Z",
        })
    );
    assert!(w.asked().is_empty(), "a read asks no one");
}

#[tokio::test]
async fn an_unset_instance_choice_is_null() {
    let w = world();
    let page = w.settings.view().await.unwrap();
    assert!(page["agent_choice"].is_null(), "{page}");
    assert_eq!(page["skills"], json!([]));
}

#[tokio::test]
async fn a_failing_section_fails_the_page() {
    let w = world_with(vec![Arc::new(FailingSection)], Referents::default());
    assert_eq!(
        w.settings.view().await.unwrap_err(),
        SettingsError::Storage("the probe broke".into())
    );
}
