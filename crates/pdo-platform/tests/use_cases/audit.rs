use std::sync::Arc;

use pdo_platform::application::list_audit::ListAudit;
use pdo_platform::application::record_audit::RecordAudit;
use pdo_platform::domain::audit::{AuditFilter, NewAuditEntry};

use pdo_extension_api::ActorDisplay;
use pdo_kernel::Actor;

use crate::fakes::{Audit, Host};

fn entry(action: &str, kind: &str) -> NewAuditEntry {
    NewAuditEntry {
        actor_hint: "ui".into(),
        action: action.into(),
        target_kind: Some(kind.into()),
        target_id: Some("t-1".into()),
        before: None,
        after: None,
    }
}

#[tokio::test]
async fn recorded_rows_come_back_newest_first_and_filtered() {
    let audit = Arc::new(Audit::default());
    let record = RecordAudit::new(audit.clone());
    record.execute(entry("trigger.created", "trigger")).await;
    record
        .execute(entry("triggers.pause_changed", "instance"))
        .await;
    let list = ListAudit::new(audit, Host::allowing());
    let all = list.execute(&AuditFilter::default()).await.unwrap();
    assert_eq!(all[0].action, "triggers.pause_changed");
    let triggers = AuditFilter {
        target_kind: Some("trigger".into()),
        ..AuditFilter::default()
    };
    let rows = list.execute(&triggers).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].action, "trigger.created");
}

#[tokio::test]
async fn a_failed_write_never_fails_the_mutation() {
    let audit = Arc::new(Audit {
        fails: true,
        ..Audit::default()
    });
    // Returns, never panics: the mutation the row belongs to already committed.
    RecordAudit::new(audit.clone())
        .execute(entry("trigger.updated", "trigger"))
        .await;
    assert!(audit.rows.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_row_carries_the_actor_of_its_request_and_its_display_name() {
    let audit = Arc::new(Audit::default());
    let record = RecordAudit::new(audit.clone());
    let alice = Actor::Identified {
        namespace: "test".into(),
        id: "alice".into(),
    };
    pdo_kernel::actor::scope(
        alice.clone(),
        record.execute(entry("trigger.updated", "trigger")),
    )
    .await;
    record.execute(entry("trigger.created", "trigger")).await;
    let rows = ListAudit::new(audit, Host::allowing())
        .execute(&AuditFilter::default())
        .await
        .unwrap();
    assert_eq!(rows[0].actor, None, "outside a request, no Actor");
    assert_eq!(rows[0].actor_display, None);
    assert_eq!(rows[1].actor, Some(alice));
    assert_eq!(rows[1].actor_display, Some(ActorDisplay::new("Alice")));
}
