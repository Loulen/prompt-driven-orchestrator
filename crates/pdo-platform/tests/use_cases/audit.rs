use std::sync::Arc;

use pdo_platform::application::list_audit::ListAudit;
use pdo_platform::application::record_audit::RecordAudit;
use pdo_platform::domain::audit::{AuditFilter, NewAuditEntry};

use crate::fakes::Audit;

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
    let list = ListAudit::new(audit);
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
