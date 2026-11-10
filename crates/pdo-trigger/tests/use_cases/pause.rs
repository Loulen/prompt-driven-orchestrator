use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use pdo_platform::domain::audit::ActorHint;
use pdo_trigger::application::TriggerError;
use serde_json::json;

use crate::fakes::world;

#[tokio::test]
async fn the_pause_flips_the_switch_announces_it_and_journals_both_sides() {
    let mut w = world();
    w.triggers
        .pause(&Actor::LocalOperator, ActorHint::Ui, true)
        .await
        .unwrap();

    assert!(*w.settings.paused.lock().unwrap());
    assert_eq!(
        w.announced(),
        vec![json!({ "type": "triggers_paused", "paused": true })]
    );
    let entries = w.audit.entries.lock().unwrap().clone();
    let e = &entries[0];
    assert_eq!(e.action, "triggers.pause_changed");
    assert_eq!(e.target_kind.as_deref(), Some("instance"));
    assert!(e.target_id.is_none(), "instance-global has no target_id");
    assert_eq!(e.before, Some(json!({ "paused": false })));
    assert_eq!(e.after, Some(json!({ "paused": true })));
    assert_eq!(
        w.host.asked.lock().unwrap().clone(),
        vec![(
            Actor::LocalOperator,
            Operation::new(ResourceKind::Trigger, Verb::Action("pause".into()))
        )]
    );
    assert!(w.triggers.health().await.paused, "the health says so");
}

#[tokio::test]
async fn a_busy_store_does_not_make_a_paused_instance_read_as_running() {
    let w = world();
    *w.settings.paused.lock().unwrap() = true;
    *w.settings.busy_reads.lock().unwrap() = 1;
    assert!(
        w.triggers.health().await.paused,
        "asked again, not failed open"
    );
}

#[tokio::test]
async fn a_refused_pause_leaves_the_switch_alone() {
    let w = world();
    w.refuse();
    assert!(matches!(
        w.triggers
            .pause(&Actor::LocalOperator, ActorHint::Ui, true)
            .await
            .unwrap_err(),
        TriggerError::Refused(_)
    ));
    assert!(!*w.settings.paused.lock().unwrap());
}
