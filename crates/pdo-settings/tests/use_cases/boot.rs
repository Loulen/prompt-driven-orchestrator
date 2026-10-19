use pdo_kernel::{Actor, ResourceKind, Verb};
use pdo_settings::application::boot::warn_about_retired_settings;
use pdo_settings::application::SettingsError;

use crate::fakes::{world, InMemoryConfig};

#[tokio::test]
async fn nothing_retired_says_nothing() {
    assert_eq!(
        warn_about_retired_settings(&InMemoryConfig::default()).await,
        None
    );
}

#[tokio::test]
async fn a_retired_value_is_said_once_naming_the_profile_runs_fall_back_to() {
    let config = InMemoryConfig::default();
    config
        .retired
        .lock()
        .unwrap()
        .push(("image_source", "dockerfile".into()));
    config.row.lock().unwrap().default_sandbox = Some("minimal".into());
    let message = warn_about_retired_settings(&config).await.unwrap();
    assert!(
        message.contains("`image_source` = `dockerfile`"),
        "{message}"
    );
    assert!(message.contains("`minimal`"), "{message}");
}

#[tokio::test]
async fn the_catalogues_warm_up_asks_no_one() {
    let w = world();
    w.settings.warm_harness_catalogues().await;
    assert_eq!(*w.harnesses.warmed.lock().unwrap(), 1);
    assert!(w.asked().is_empty());
}

#[tokio::test]
async fn a_refresh_probes_the_harness_again_and_announces_it() {
    let w = world();
    let entry = w
        .settings
        .refresh_harness_catalogue(&Actor::LocalOperator, "claude")
        .await
        .unwrap();
    assert_eq!(entry["name"], "claude");
    assert_eq!(*w.harnesses.refreshed.lock().unwrap(), vec!["claude"]);
    assert_eq!(*w.harnesses.announced.lock().unwrap(), vec!["claude"]);
    let asked = w.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].1.resource, ResourceKind::InstanceSettings);
    assert_eq!(asked[0].1.verb, Verb::Action("refresh_catalogue".into()));
    assert_eq!(asked[0].1.target.as_deref(), Some("claude"));
}

#[tokio::test]
async fn refreshing_an_unknown_harness_is_not_found_and_announces_nothing() {
    let w = world();
    let outcome = w
        .settings
        .refresh_harness_catalogue(&Actor::LocalOperator, "ghost")
        .await;
    assert!(
        matches!(&outcome, Err(SettingsError::NotFound(m)) if m.contains("ghost")),
        "{outcome:?}"
    );
    assert!(w.harnesses.announced.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_refused_refresh_probes_nothing() {
    let w = world();
    w.refuse();
    let outcome = w
        .settings
        .refresh_harness_catalogue(&Actor::LocalOperator, "claude")
        .await;
    assert!(
        matches!(outcome, Err(SettingsError::Refused(_))),
        "{outcome:?}"
    );
    assert!(w.harnesses.refreshed.lock().unwrap().is_empty());
    assert!(w.harnesses.announced.lock().unwrap().is_empty());
}
