use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use pdo_platform::domain::audit::ActorHint;
use pdo_trigger::application::TriggerError;
use serde_json::json;

use crate::fakes::{read_guard, request, world, PIPE, REPO, TARGET_REPO_REQUIRED};

#[tokio::test]
async fn a_creation_stores_announces_and_journals_the_full_trigger() {
    let mut w = world();
    let trigger = w
        .triggers
        .create(&Actor::LocalOperator, ActorHint::Ui, request(json!({})))
        .await
        .unwrap();

    assert_eq!(trigger.pipeline_name, PIPE);
    assert_eq!(trigger.target_repo.as_deref(), Some(REPO));
    assert!(
        trigger.next_fire_at.is_some(),
        "the first fire is scheduled"
    );
    assert_eq!(w.store.all(), vec![trigger.clone()]);
    assert_eq!(
        w.announced(),
        vec![
            json!({ "type": "trigger_created", "trigger_id": trigger.id, "read_guard": read_guard(&trigger.id) })
        ]
    );
    let entries = w.audit.entries.lock().unwrap().clone();
    assert_eq!(entries.len(), 1, "one audit row");
    let e = &entries[0];
    assert_eq!(e.action, "trigger.created");
    assert_eq!(e.actor_hint, "ui");
    assert_eq!(e.target_kind.as_deref(), Some("trigger"));
    assert_eq!(e.target_id.as_deref(), Some(trigger.id.as_str()));
    assert!(e.before.is_none(), "a creation has no before");
    assert_eq!(e.after.as_ref().unwrap()["name"], "T", "the FULL trigger");
    assert_eq!(
        w.host.asked.lock().unwrap().clone(),
        vec![(
            Actor::LocalOperator,
            Operation::new(ResourceKind::Trigger, Verb::Create)
        )]
    );
}

#[tokio::test]
async fn blanks_are_normalised_and_the_overlap_policy_is_one_of_two() {
    let w = world();
    let id = w
        .create(request(json!({
            "guard_command": "  ",
            "sandbox": "",
            "harness": " ",
            "overlap_policy": "bogus",
        })))
        .await;
    let t = w.store.one(&id).unwrap();
    assert_eq!(t.guard_command, None);
    assert_eq!(t.sandbox, None);
    assert_eq!(t.harness, None);
    assert_eq!(t.overlap_policy, "skip");
}

async fn refused(extra: serde_json::Value) -> String {
    let w = world();
    let err = w
        .triggers
        .create(&Actor::LocalOperator, ActorHint::Ui, request(extra))
        .await
        .unwrap_err();
    assert!(w.store.all().is_empty(), "nothing is stored on a refusal");
    match err {
        TriggerError::Invalid(message) => message,
        other => panic!("expected a 400, got {other:?}"),
    }
}

#[tokio::test]
async fn every_invalid_configuration_is_a_400_that_stores_nothing() {
    assert_eq!(
        refused(json!({ "name": "  " })).await,
        "trigger name must not be empty"
    );
    assert!(refused(json!({ "cron": "* *" }))
        .await
        .starts_with("invalid cron expression: "));
    assert_eq!(
        refused(json!({ "max_concurrent": 0 })).await,
        "max_concurrent must be >= 1"
    );
    assert!(refused(json!({ "sandbox": "ghost" }))
        .await
        .contains("unknown sandbox profile"));
    assert_eq!(
        refused(json!({ "pipeline_id": "ghost-pipe" })).await,
        "pipeline not found: ghost-pipe"
    );
    assert!(
        refused(json!({ "pipeline_id": "needs-prompt", "input_template": "" }))
            .await
            .starts_with("this pipeline requires a prompt")
    );
    assert_eq!(
        refused(json!({ "target_repo": null })).await,
        TARGET_REPO_REQUIRED
    );
}

#[tokio::test]
async fn a_broken_pipeline_document_is_a_parse_error() {
    let w = world();
    w.pipelines.broken.lock().unwrap().push("broken".into());
    let err = w
        .triggers
        .create(
            &Actor::LocalOperator,
            ActorHint::Ui,
            request(json!({ "pipeline_id": "broken" })),
        )
        .await
        .unwrap_err();
    assert_eq!(
        err,
        TriggerError::Invalid("pipeline parse error: bad yaml".into())
    );
}

#[tokio::test]
async fn a_refused_creation_checks_and_stores_nothing() {
    let mut w = world();
    w.refuse();
    // Even an invalid request is refused first: the refusal comes before any check.
    let err = w
        .triggers
        .create(
            &Actor::LocalOperator,
            ActorHint::Ui,
            request(json!({ "name": "" })),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, TriggerError::Refused(_)), "{err:?}");
    assert!(w.store.all().is_empty());
    assert!(w.announced().is_empty());
    assert!(w.audit.entries.lock().unwrap().is_empty());
}
