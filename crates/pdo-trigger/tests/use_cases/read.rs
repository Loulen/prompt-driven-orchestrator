use pdo_trigger::application::TriggerError;

use crate::fakes::{world, REPO};

#[tokio::test]
async fn the_list_resolves_the_effective_repo_without_touching_the_raw_one() {
    let w = world();
    let id = w.create_standard().await;
    let legacy = w.create_standard().await;
    w.store
        .triggers
        .lock()
        .unwrap()
        .iter_mut()
        .filter(|t| t.id == legacy)
        .for_each(|t| t.target_repo = None);

    let list = w.triggers.list().await.unwrap();
    let of = |id: &str| list.iter().find(|e| e.trigger.id == id).unwrap();
    assert_eq!(of(&id).effective_repo, REPO);
    assert_eq!(of(&legacy).effective_repo, "/daemon/root");
    assert_eq!(of(&legacy).trigger.target_repo, None);
    let wire = serde_json::to_value(of(&id)).unwrap();
    assert_eq!(wire["effective_repo"], REPO, "flattened beside the Trigger");
    assert_eq!(wire["id"], serde_json::json!(id));
}

#[tokio::test]
async fn one_trigger_its_fires_and_the_health() {
    let w = world();
    assert_eq!(
        w.triggers.get("ghost").await.unwrap_err(),
        TriggerError::NotFound("trigger not found".into())
    );
    let id = w.create_standard().await;
    assert_eq!(w.triggers.get(&id).await.unwrap().name, "T");
    assert!(w.triggers.fires(&id).await.unwrap().is_empty());

    let health = w.triggers.health().await;
    assert_eq!(health.last_tick_at, None, "never ticked");
    assert_eq!(health.tick_interval_secs, 30);
    assert!(!health.paused);
}
