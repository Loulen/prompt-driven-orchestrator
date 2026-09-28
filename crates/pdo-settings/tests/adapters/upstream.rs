//! Stage 3 (ADR-0080): the ports the upstream contexts read their instance
//! settings through, over the real stored tier — read fresh, so an edit takes
//! effect on the next read — and who names a profile, over the real disk.

use std::sync::Arc;
use std::time::Duration;

use pdo_platform::application::ports::UpdateCheckSetting;
use pdo_review::application::ports::ReviewSettings;
use pdo_run::application::ports::InstanceSettings;
use pdo_settings::adapters::contexts::{
    ContextReferents, RunInstanceSettings, SettingsOfReview, SettingsOfTriggers,
    UpdateCheckOfPlatform,
};
use pdo_settings::adapters::sqlite::{instance_config, SqliteAgentProfiles, SqliteInstanceConfig};
use pdo_settings::application::ports::ProfileReferents;
use pdo_settings::domain::instance_config::UpdateInstanceConfig;
use pdo_trigger::application::ports::TriggerSettings;

use crate::instance_config::test_db;

#[tokio::test]
async fn a_run_reads_the_stored_defaults_and_the_profiles_fresh() {
    let db = test_db().await;
    let settings = RunInstanceSettings {
        config: Arc::new(SqliteInstanceConfig::new(db.clone())),
        profiles: Arc::new(SqliteAgentProfiles::new(db.clone())),
    };
    instance_config::update(
        &db,
        UpdateInstanceConfig {
            session_cap: Some(3),
            auto_fail: Some(true),
            manager_enabled: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let defaults = settings.defaults().await.unwrap();
    assert_eq!(defaults.session_cap, Some(3));
    assert!(defaults.auto_fail, "a stored 1 resolves to on");
    assert!(!defaults.manager_enabled, "a stored 0 resolves to off");

    let profiles = settings.agent_profiles().await;
    assert_eq!(profiles["default"].harness, "claude");
    let found = settings.profile_named("DEFAULT").await.unwrap();
    assert_eq!(found.unwrap().harness, "claude");
    assert_eq!(settings.profile_named("ghost").await.unwrap(), None);
}

#[tokio::test]
async fn the_triggers_pause_round_trips_and_the_guard_timeout_follows_the_stored_tier() {
    let db = test_db().await;
    let settings = SettingsOfTriggers(Arc::new(SqliteInstanceConfig::new(db.clone())));
    assert!(!settings.paused().await.unwrap());
    settings.set_paused(true).await.unwrap();
    assert!(settings.paused().await.unwrap());
    instance_config::update(
        &db,
        UpdateInstanceConfig {
            guard_timeout_secs: Some(30),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(settings.guard_timeout().await, Duration::from_secs(30));
}

#[tokio::test]
async fn a_stored_decision_beats_the_env_for_the_review_and_the_version_check() {
    let db = test_db().await;
    let config = Arc::new(SqliteInstanceConfig::new(db.clone()));
    instance_config::update(
        &db,
        UpdateInstanceConfig {
            review_agent_can_resolve: Some(true),
            update_check: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(SettingsOfReview(config.clone()).agent_can_resolve().await);
    assert!(!UpdateCheckOfPlatform(config).enabled().await);
}

#[tokio::test]
async fn a_pipeline_node_naming_the_profile_is_a_referent() {
    let db = test_db().await;
    pdo_workspace::adapters::sqlite::init(&db).await.unwrap();
    pdo_trigger::adapters::sqlite::init(&db).await.unwrap();
    let repo = tempfile::tempdir().unwrap();
    let pipelines = repo.path().join(".pdo").join("pipelines");
    std::fs::create_dir_all(&pipelines).unwrap();
    std::fs::write(pipelines.join("profiled.yaml"), PROFILED).unwrap();
    let referents = ContextReferents {
        db,
        repo_root: repo.path().to_path_buf(),
    };
    let found = referents.pipelines("agp-1");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        (found[0].id.as_str(), found[0].node_id.as_str()),
        ("profiled", "worker")
    );
    assert_eq!(found[0].scope, "instance");
    assert!(referents.pipelines("agp-2").is_empty());
    assert!(referents.projects("agp-1").await.is_empty());
    assert!(referents.triggers("agp-1").await.is_empty());
}

/// A pipeline whose worker node names the profile `agp-1`.
const PROFILED: &str = r#"name: profiled
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: Worker
    type: agent
    agent_choice: { mode: profile, profile_id: agp-1 }
    inputs:
      - name: task
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: out }
    target: { node: end, port: result }
"#;
