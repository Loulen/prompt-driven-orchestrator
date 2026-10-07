//! API 0.8: a Trigger records its last editor, and an extension may suspend
//! it — it then fires neither on its schedule nor by "Run now", and every read
//! says why.

use pdo_kernel::Actor;
use pdo_platform::domain::audit::ActorHint;
use pdo_trigger::application::fire::tick::SKIPPED_SUSPENDED;
use pdo_trigger::application::update_trigger::PatchTriggerRequest;
use pdo_trigger::application::TriggerError;
use pdo_trigger::domain::trigger::Suspension;
use serde_json::json;

use crate::fakes::{request, world, World};

fn alice() -> Actor {
    Actor::Identified {
        namespace: "test".into(),
        id: "alice".into(),
    }
}

fn make_due(w: &World, id: &str) {
    w.store
        .triggers
        .lock()
        .unwrap()
        .iter_mut()
        .filter(|t| t.id == id)
        .for_each(|t| t.next_fire_at = Some("2020-01-01T00:00:00.000Z".into()));
}

fn patch(body: serde_json::Value) -> PatchTriggerRequest {
    serde_json::from_value(body).unwrap()
}

#[tokio::test]
async fn the_creator_is_the_first_editor_and_each_edit_takes_over() {
    let w = world();
    let created = w
        .triggers
        .create(&alice(), ActorHint::Ui, request(json!({})))
        .await
        .unwrap();
    assert_eq!(created.last_editor, Some(alice()));
    assert_eq!(w.store.one(&created.id).unwrap().last_editor, Some(alice()));

    let bob = Actor::Identified {
        namespace: "test".into(),
        id: "bob".into(),
    };
    let updated = w
        .triggers
        .update(
            &bob,
            ActorHint::Ui,
            &created.id,
            patch(json!({ "enabled": false })),
        )
        .await
        .unwrap();
    assert_eq!(
        updated.last_editor,
        Some(bob.clone()),
        "a toggle is an edit"
    );
    assert_eq!(w.store.one(&created.id).unwrap().last_editor, Some(bob));
}

#[tokio::test]
async fn the_extension_is_asked_with_the_trigger_and_its_last_editor() {
    let w = world();
    let created = w
        .triggers
        .create(
            &alice(),
            ActorHint::Ui,
            request(json!({ "name": "Nightly" })),
        )
        .await
        .unwrap();
    w.host.suspension_asks.lock().unwrap().clear();
    let listed = w.triggers.list(Default::default()).await.unwrap();
    assert_eq!(listed[0].trigger.suspension, None);
    let asked = w.host.suspension_asks.lock().unwrap().clone();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].id, created.id);
    assert_eq!(asked[0].name, "Nightly");
    assert_eq!(asked[0].last_editor, Some(alice()));
}

#[tokio::test]
async fn every_read_carries_the_suspension_and_its_reason() {
    let w = world();
    let id = w.create_standard().await;
    w.suspend(&id, "alice is on leave");
    let expected = Some(Suspension {
        extension: "test".into(),
        reason: "alice is on leave".into(),
    });

    let listed = w.triggers.list(Default::default()).await.unwrap();
    assert_eq!(listed[0].trigger.suspension, expected);
    assert_eq!(
        w.triggers.attributed(&id).await.unwrap().suspension,
        expected
    );
    assert_eq!(
        w.store.one(&id).unwrap().suspension,
        None,
        "never stored: the extension decides at every read"
    );
}

#[tokio::test]
async fn a_suspended_trigger_spends_its_slot_without_firing_and_says_why() {
    let w = world();
    let id = w.create_standard().await;
    make_due(&w, &id);
    w.suspend(&id, "alice is on leave");

    w.triggers.tick().await;

    assert!(w.runs.started.lock().unwrap().is_empty(), "no Run");
    assert!(w.guard.ran.lock().unwrap().is_empty(), "no guard");
    let fire = &w.store.fires_of(&id)[0];
    assert_eq!(fire.outcome, SKIPPED_SUSPENDED);
    assert_eq!(fire.reason.as_deref(), Some("alice is on leave"));
    let next = w.store.one(&id).unwrap().next_fire_at.unwrap();
    assert!(next.as_str() > "2020-01-01", "the slot is spent: {next}");

    // Lifted: it fires again at its next slot.
    w.host.suspended.lock().unwrap().clear();
    make_due(&w, &id);
    w.triggers.tick().await;
    assert_eq!(w.runs.started.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn run_now_refuses_a_suspended_trigger_with_its_reason() {
    let w = world();
    let id = w.create_standard().await;
    w.suspend(&id, "alice is on leave");

    let refusal = w.triggers.run_now(&Actor::LocalOperator, &id).await;

    match refusal {
        Err(TriggerError::Conflict(message)) => {
            assert!(message.contains("suspended"), "{message}");
            assert!(message.contains("alice is on leave"), "{message}");
        }
        other => panic!("expected a 409, got {other:?}"),
    }
    assert!(w.runs.started.lock().unwrap().is_empty());
    assert!(w.store.fires_of(&id).is_empty(), "no fire record");
}

#[tokio::test]
async fn a_fire_hands_the_trigger_and_its_last_editor_to_the_run() {
    let w = world();
    let created = w
        .triggers
        .create(&alice(), ActorHint::Ui, request(json!({})))
        .await
        .unwrap();
    w.triggers
        .run_now(&Actor::LocalOperator, &created.id)
        .await
        .unwrap();
    let started = w.runs.started.lock().unwrap().clone();
    assert_eq!(started[0].trigger.last_editor, Some(alice()));
}
