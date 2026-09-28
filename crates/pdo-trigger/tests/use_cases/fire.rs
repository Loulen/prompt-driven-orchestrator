//! "Run now": a first-class fire, asked as the caller.

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use pdo_trigger::application::TriggerError;
use pdo_trigger::domain::fire_decision::GuardResult;
use serde_json::json;

use crate::fakes::{request, world, REPO};

fn fire(id: &str) -> Operation {
    Operation::new(ResourceKind::Trigger, Verb::Action("fire".into())).on(id)
}

#[tokio::test]
async fn run_now_starts_the_run_records_it_and_leaves_the_schedule_alone() {
    let mut w = world();
    let id = w.create_standard().await;
    let scheduled = w.store.one(&id).unwrap().next_fire_at;
    w.announced();

    let record = w
        .triggers
        .run_now(&Actor::LocalOperator, &id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.outcome, "fired");
    assert_eq!(record.run_id.as_deref(), Some("run-1"));
    assert_eq!(record.source.as_deref(), Some("manual"));

    let started = w.runs.started.lock().unwrap().clone();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].trigger.id, id);
    assert_eq!(started[0].input, "x", "the template is the input");
    assert_eq!(
        w.repos.fetched.lock().unwrap().clone(),
        vec![vec![REPO.to_string()]],
        "the remotes are refreshed before the cut"
    );
    assert_eq!(w.store.one(&id).unwrap().next_fire_at, scheduled);
    assert_eq!(w.store.fires_of(&id)[0].outcome, "fired");
    assert_eq!(
        w.announced(),
        vec![
            json!({ "type": "trigger_fired", "trigger_id": id, "outcome": "fired", "run_id": "run-1" })
        ]
    );
    assert_eq!(
        w.host.asked.lock().unwrap().last().cloned(),
        Some((Actor::LocalOperator, fire(&id)))
    );
}

#[tokio::test]
async fn a_disabled_or_dangling_trigger_is_a_409_with_no_record() {
    let w = world();
    let id = w.create_standard().await;
    w.store
        .triggers
        .lock()
        .unwrap()
        .iter_mut()
        .for_each(|t| t.enabled = false);
    assert_eq!(
        w.triggers
            .run_now(&Actor::LocalOperator, &id)
            .await
            .unwrap_err(),
        TriggerError::Conflict(format!("trigger '{id}' is disabled — enable it first"))
    );

    let legacy = w.create_standard().await;
    w.store
        .triggers
        .lock()
        .unwrap()
        .iter_mut()
        .filter(|t| t.id == legacy)
        .for_each(|t| t.target_repo = None);
    let err = w
        .triggers
        .run_now(&Actor::LocalOperator, &legacy)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, TriggerError::Conflict(m) if m.starts_with("broken reference: ")),
        "{err:?}"
    );
    assert!(w.store.fires.lock().unwrap().is_empty());
    assert!(
        w.guard.ran.lock().unwrap().is_empty(),
        "the guard never ran"
    );
}

#[tokio::test]
async fn a_guard_skip_is_an_honest_record_not_an_error() {
    let w = world();
    let id = w
        .create(request(json!({ "guard_command": "gh issue list" })))
        .await;
    *w.guard.answer.lock().unwrap() = GuardResult::Skip {
        stdout: String::new(),
        stderr: "nothing to do\n".into(),
        exit_code: Some(1),
    };
    let record = w
        .triggers
        .run_now(&Actor::LocalOperator, &id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.outcome, "guard-exit-nonzero");
    assert_eq!(record.guard_stderr.as_deref(), Some("nothing to do\n"));
    assert!(w.runs.started.lock().unwrap().is_empty());
}

#[tokio::test]
async fn the_overlap_gate_skips_before_the_guard_runs() {
    let w = world();
    let id = w
        .create(request(json!({ "guard_command": "gh issue list" })))
        .await;
    w.runs.live.lock().unwrap().push((id.clone(), 1));
    let record = w
        .triggers
        .run_now(&Actor::LocalOperator, &id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.outcome, "skipped-overlap");
    assert!(w.guard.ran.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_run_the_run_context_refuses_is_an_error_record() {
    let w = world();
    let id = w.create_standard().await;
    *w.runs.refusal.lock().unwrap() = Some("unknown sandbox profile `ghost`".into());
    let record = w
        .triggers
        .run_now(&Actor::LocalOperator, &id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.outcome, "error");
    assert_eq!(
        record.reason.as_deref(),
        Some("unknown sandbox profile `ghost`")
    );
}

#[tokio::test]
async fn a_refused_run_now_has_no_effect_and_an_unknown_one_is_a_404() {
    let w = world();
    assert_eq!(
        w.triggers
            .run_now(&Actor::LocalOperator, "ghost")
            .await
            .unwrap_err(),
        TriggerError::NotFound("trigger not found: ghost".into())
    );
    let id = w.create_standard().await;
    w.refuse();
    assert!(matches!(
        w.triggers
            .run_now(&Actor::LocalOperator, &id)
            .await
            .unwrap_err(),
        TriggerError::Refused(_)
    ));
    assert!(w.runs.started.lock().unwrap().is_empty());
    assert!(w.store.fires.lock().unwrap().is_empty());
}
