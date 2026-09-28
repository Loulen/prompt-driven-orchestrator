use pdo_settings::application::boot::warn_about_retired_settings;

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
