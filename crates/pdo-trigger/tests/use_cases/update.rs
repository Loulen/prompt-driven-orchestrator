use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use pdo_platform::domain::audit::ActorHint;
use pdo_trigger::application::update_trigger::PatchTriggerRequest;
use pdo_trigger::application::TriggerError;
use pdo_trigger::domain::trigger::Trigger;
use pdo_trigger::domain::trigger::UpdateTrigger;
use serde_json::json;

use crate::fakes::{world, World, REPO};

fn patch(body: serde_json::Value) -> PatchTriggerRequest {
    serde_json::from_value(body).unwrap()
}

async fn update(w: &World, id: &str, body: serde_json::Value) -> Result<Trigger, TriggerError> {
    w.triggers
        .update(&Actor::LocalOperator, ActorHint::Ui, id, patch(body))
        .await
}

#[tokio::test]
async fn a_repoint_moves_the_id_and_the_name_and_revives_the_next_fire() {
    let w = world();
    let id = w.create_standard().await;
    // Dormant, as a Trigger whose pipeline was renamed away.
    w.store
        .triggers
        .lock()
        .unwrap()
        .iter_mut()
        .for_each(|t| t.next_fire_at = None);

    let updated = update(&w, &id, json!({ "pipeline_id": "weekly-pipe" }))
        .await
        .unwrap();
    assert_eq!(updated.pipeline_id, "weekly-pipe");
    assert_eq!(updated.pipeline_name, "Weekly");
    let stored = w.store.one(&id).unwrap();
    assert_eq!(stored, updated, "persisted, not merely echoed back");
    assert!(stored.next_fire_at.is_some(), "revived from its own cron");
}

#[tokio::test]
async fn a_repoint_to_an_unknown_pipeline_is_a_400_that_changes_nothing() {
    let w = world();
    let id = w.create_standard().await;
    let before = w.store.one(&id).unwrap();
    let err = update(&w, &id, json!({ "pipeline_id": "ghost-pipe" }))
        .await
        .unwrap_err();
    assert_eq!(
        err,
        TriggerError::Invalid("pipeline not found: ghost-pipe".into())
    );
    assert_eq!(w.store.one(&id).unwrap(), before);
}

#[tokio::test]
async fn a_disable_journals_before_true_and_after_false() {
    let mut w = world();
    let id = w.create_standard().await;
    w.announced();
    update(&w, &id, json!({ "enabled": false })).await.unwrap();

    let entries = w.audit.entries.lock().unwrap().clone();
    let e = entries
        .iter()
        .find(|e| e.action == "trigger.updated")
        .unwrap();
    assert_eq!(e.actor_hint, "ui");
    assert_eq!(e.before.as_ref().unwrap()["enabled"], true);
    assert_eq!(e.after.as_ref().unwrap()["enabled"], false);
    assert_eq!(
        w.announced(),
        vec![json!({ "type": "trigger_updated", "trigger_id": id })]
    );
    assert_eq!(
        w.host.asked.lock().unwrap().last().cloned(),
        Some((
            Actor::LocalOperator,
            Operation::new(ResourceKind::Trigger, Verb::Update).on(id.clone())
        ))
    );
}

#[tokio::test]
async fn clearing_or_blanking_the_target_repo_is_refused() {
    let w = world();
    let id = w.create_standard().await;
    for body in [
        json!({ "target_repo": null }),
        json!({ "target_repo": "" }),
        json!({ "target_repo": "   " }),
    ] {
        let err = update(&w, &id, body.clone()).await.unwrap_err();
        assert!(
            matches!(&err, TriggerError::Invalid(m) if m.contains("target_repo")),
            "{body}: {err:?}"
        );
        assert_eq!(w.store.one(&id).unwrap().target_repo.as_deref(), Some(REPO));
    }
}

#[tokio::test]
async fn an_absent_target_repo_leaves_it_untouched() {
    // PATCH is a partial MERGE: that is what makes `{"enabled": false}` from the
    // list toggle a legal body.
    let w = world();
    let id = w.create_standard().await;
    let updated = update(&w, &id, json!({ "enabled": false })).await.unwrap();
    assert!(!updated.enabled);
    assert_eq!(updated.target_repo.as_deref(), Some(REPO));
}

#[tokio::test]
async fn re_enabling_recomputes_the_next_fire_forward() {
    let w = world();
    let id = w.create_standard().await;
    update(&w, &id, json!({ "enabled": false })).await.unwrap();
    w.store
        .triggers
        .lock()
        .unwrap()
        .iter_mut()
        .for_each(|t| t.next_fire_at = Some("2020-01-01T00:00:00.000Z".into()));

    let updated = update(&w, &id, json!({ "enabled": true })).await.unwrap();
    let next = updated.next_fire_at.unwrap();
    assert!(
        next.as_str() > pdo_kernel::clock::now_iso().as_str(),
        "{next}"
    );
}

#[tokio::test]
async fn the_create_time_rules_hold_on_the_resulting_configuration() {
    let w = world();
    let id = w.create_standard().await;
    let prompt = update(
        &w,
        &id,
        json!({ "pipeline_id": "needs-prompt", "input_template": "" }),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(prompt, TriggerError::Invalid(m) if m.starts_with("this pipeline requires a prompt"))
    );
    assert_eq!(
        update(&w, &id, json!({ "max_concurrent": 0 }))
            .await
            .unwrap_err(),
        TriggerError::Invalid("max_concurrent must be >= 1".into())
    );
    assert!(matches!(
        update(&w, &id, json!({ "sandbox": "ghost" }))
            .await
            .unwrap_err(),
        TriggerError::Invalid(_)
    ));
    assert_eq!(
        update(&w, &id, json!({ "name": " " })).await.unwrap_err(),
        TriggerError::Invalid("trigger name must not be empty".into())
    );
    assert!(matches!(
        update(&w, &id, json!({ "cron": "nope" })).await.unwrap_err(),
        TriggerError::Invalid(m) if m.starts_with("invalid cron expression")
    ));
}

#[tokio::test]
async fn present_nulls_clear_and_absent_fields_stay() {
    let w = world();
    let id = w
        .create(crate::fakes::request(
            json!({ "harness": "codex", "max_concurrent": 3, "overlap_policy": "allow" }),
        ))
        .await;
    let updated = update(&w, &id, json!({ "harness": null, "max_concurrent": null }))
        .await
        .unwrap();
    assert_eq!(updated.harness, None);
    assert_eq!(updated.max_concurrent, None);
    assert_eq!(updated.overlap_policy, "allow");
}

#[tokio::test]
async fn an_unknown_trigger_is_a_404_and_a_refused_edit_changes_nothing() {
    let w = world();
    assert_eq!(
        update(&w, "ghost", json!({ "enabled": false }))
            .await
            .unwrap_err(),
        TriggerError::NotFound("trigger not found".into())
    );
    let id = w.create_standard().await;
    let before = w.store.one(&id).unwrap();
    w.refuse();
    assert!(matches!(
        update(&w, &id, json!({ "enabled": false }))
            .await
            .unwrap_err(),
        TriggerError::Refused(_)
    ));
    assert_eq!(w.store.one(&id).unwrap(), before);
}

#[test]
fn an_empty_edit_is_empty() {
    assert!(UpdateTrigger::default().is_empty());
}
