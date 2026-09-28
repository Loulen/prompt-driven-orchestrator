use pdo_kernel::Actor;
use pdo_platform::domain::audit::ActorHint;
use pdo_trigger::application::TriggerError;
use serde_json::json;

use crate::fakes::world;

#[tokio::test]
async fn a_deletion_journals_the_whole_row_it_removed() {
    // `delete` returns a bool, never the row: the use case must read the row
    // BEFORE deleting or the audit degrades to a dead id.
    let mut w = world();
    let id = w.create_standard().await;
    w.announced();
    w.triggers
        .delete(&Actor::LocalOperator, ActorHint::Ui, &id)
        .await
        .unwrap();

    assert!(w.store.one(&id).is_none());
    assert_eq!(
        w.announced(),
        vec![json!({ "type": "trigger_deleted", "trigger_id": id })]
    );
    let entries = w.audit.entries.lock().unwrap().clone();
    let e = entries
        .iter()
        .find(|e| e.action == "trigger.deleted")
        .unwrap();
    assert_eq!(e.actor_hint, "ui");
    assert!(e.after.is_none());
    let before = e.before.as_ref().unwrap();
    assert_eq!(before["id"], json!(id));
    assert_eq!(before["cron"], "0 4 * * 1");
}

#[tokio::test]
async fn deleting_an_unknown_trigger_is_a_404() {
    let w = world();
    assert_eq!(
        w.triggers
            .delete(&Actor::LocalOperator, ActorHint::Ui, "ghost")
            .await
            .unwrap_err(),
        TriggerError::NotFound("trigger not found".into())
    );
}

#[tokio::test]
async fn a_refused_deletion_keeps_the_trigger() {
    let w = world();
    let id = w.create_standard().await;
    w.refuse();
    assert!(matches!(
        w.triggers
            .delete(&Actor::LocalOperator, ActorHint::Ui, &id)
            .await
            .unwrap_err(),
        TriggerError::Refused(_)
    ));
    assert!(w.store.one(&id).is_some());
}
