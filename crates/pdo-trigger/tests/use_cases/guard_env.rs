//! A Trigger's guard acts for whoever answers for the Trigger: it runs with the
//! environment the extensions give its last editor in its Projet (the
//! dry-run: who asks it), and the extensions are told when it ends, whatever
//! its end, to revoke what they handed it. Without an extension, nothing is
//! added.

use std::collections::BTreeMap;

use pdo_kernel::extension_host::{GitOperationKind, GuardOutcome};
use pdo_kernel::Actor;
use pdo_platform::domain::audit::ActorHint;
use pdo_trigger::application::test_guard::GuardTestRequest;
use pdo_trigger::domain::fire_decision::GuardResult;
use serde_json::json;

use crate::fakes::{request, world, World, REPO};

fn alice() -> Actor {
    Actor::Identified {
        namespace: "test".into(),
        id: "alice".into(),
    }
}

fn token() -> BTreeMap<String, String> {
    BTreeMap::from([("X_TEST_TOKEN".to_string(), "tok-1".to_string())])
}

/// A Trigger alice last edited, guarded, launching in `p1`: its id.
async fn guarded_by_alice(w: &World) -> String {
    w.projets
        .members
        .lock()
        .unwrap()
        .push((REPO.into(), "p1".into()));
    w.triggers
        .create(
            &alice(),
            ActorHint::Ui,
            request(json!({ "guard_command": "gh issue list", "project_id": "p1" })),
        )
        .await
        .unwrap()
        .id
}

#[tokio::test]
async fn a_fire_s_guard_reads_what_the_extensions_give_its_last_editor_in_its_projet() {
    let w = world();
    *w.host.guard_env.lock().unwrap() = token();
    let id = guarded_by_alice(&w).await;

    let record = w
        .triggers
        .run_now(&Actor::LocalOperator, &id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(record.outcome, "fired", "the Run starts");
    assert_eq!(w.guard.envs.lock().unwrap().clone(), vec![token()]);
    let asks = w.host.git_asks.lock().unwrap().clone();
    let guard = asks
        .iter()
        .find(|op| op.kind == GitOperationKind::Guard)
        .expect("the guard's environment was asked");
    assert_eq!(guard.actor, alice(), "its last editor, not who clicked");
    assert_eq!(guard.project.as_deref(), Some("p1"));
    assert_eq!(guard.url, REPO);
    let run = guard.guard.clone().unwrap();
    assert_eq!(run.trigger_id.as_deref(), Some(id.as_str()));

    let ends = w.host.guard_ends.lock().unwrap().clone();
    assert_eq!(ends.len(), 1, "told once");
    assert_eq!(ends[0].guard, run, "the same execution");
    assert_eq!(ends[0].actor, alice());
    assert_eq!(ends[0].project.as_deref(), Some("p1"));
    assert_eq!(ends[0].outcome, GuardOutcome::Passed);
}

#[tokio::test]
async fn the_extensions_hear_every_end_of_a_guard() {
    let cases = [
        (
            GuardResult::Skip {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: Some(1),
            },
            GuardOutcome::Skipped,
        ),
        (
            GuardResult::Error {
                detail: "guard timed out after 10ms".into(),
                timed_out: true,
            },
            GuardOutcome::TimedOut,
        ),
        (
            GuardResult::Error {
                detail: "failed to spawn guard".into(),
                timed_out: false,
            },
            GuardOutcome::Failed,
        ),
    ];
    for (answer, outcome) in cases {
        let w = world();
        *w.host.guard_env.lock().unwrap() = token();
        let id = guarded_by_alice(&w).await;
        *w.guard.answer.lock().unwrap() = answer;
        w.triggers
            .run_now(&Actor::LocalOperator, &id)
            .await
            .unwrap()
            .unwrap();
        let ends = w.host.guard_ends.lock().unwrap().clone();
        assert_eq!(ends.len(), 1);
        assert_eq!(ends[0].outcome, outcome);
        assert!(w.runs.started.lock().unwrap().is_empty(), "no Run");
    }
}

#[tokio::test]
async fn without_an_extension_s_variables_the_guard_runs_as_before() {
    let w = world();
    let id = guarded_by_alice(&w).await;
    w.triggers
        .run_now(&Actor::LocalOperator, &id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(w.guard.envs.lock().unwrap().clone(), vec![BTreeMap::new()]);
}

#[tokio::test]
async fn a_trigger_without_a_guard_asks_nothing_of_its_guard() {
    let w = world();
    let id = w.create_standard().await;
    w.triggers
        .run_now(&Actor::LocalOperator, &id)
        .await
        .unwrap()
        .unwrap();
    assert!(w
        .host
        .git_asks
        .lock()
        .unwrap()
        .iter()
        .all(|op| op.kind != GitOperationKind::Guard));
    assert!(w.host.guard_ends.lock().unwrap().is_empty());
}

#[tokio::test]
async fn the_dry_run_acts_for_who_asks_it_in_the_projet_it_names() {
    let w = world();
    *w.host.guard_env.lock().unwrap() = token();
    w.triggers
        .test_guard(
            &alice(),
            GuardTestRequest {
                guard_command: "gh issue list".into(),
                target_repo: Some(REPO.into()),
                project_id: Some("p1".into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(w.guard.envs.lock().unwrap().clone(), vec![token()]);
    let ends = w.host.guard_ends.lock().unwrap().clone();
    assert_eq!(ends.len(), 1);
    assert_eq!(ends[0].actor, alice());
    assert_eq!(ends[0].project.as_deref(), Some("p1"));
    assert_eq!(ends[0].guard.trigger_id, None, "no Trigger saved yet");
    assert_eq!(ends[0].outcome, GuardOutcome::Passed);
}

#[tokio::test]
async fn a_guard_dropped_with_its_request_still_ends_for_the_extensions() {
    let w = world();
    *w.host.guard_env.lock().unwrap() = token();
    *w.guard.hangs.lock().unwrap() = true;
    let alice = alice();
    let dry_run = w.triggers.test_guard(
        &alice,
        GuardTestRequest {
            guard_command: "gh issue list".into(),
            target_repo: Some(REPO.into()),
            project_id: None,
        },
    );
    let dropped = tokio::time::timeout(std::time::Duration::from_millis(20), dry_run).await;
    assert!(dropped.is_err(), "the guard never answered");
    let ends = w.host.guard_ends.lock().unwrap().clone();
    assert_eq!(ends.len(), 1, "told once, on the drop");
    assert_eq!(ends[0].outcome, GuardOutcome::Failed);
}

#[tokio::test]
async fn the_dry_run_is_judged_in_the_projet_it_names() {
    let w = world();
    w.triggers
        .test_guard(
            &alice(),
            GuardTestRequest {
                guard_command: "true".into(),
                target_repo: Some(REPO.into()),
                project_id: Some("p1".into()),
            },
        )
        .await
        .unwrap();
    let asked = w.host.asked.lock().unwrap().clone();
    let (_, op) = asked.last().unwrap();
    assert_eq!(op.projects, vec!["p1".to_string()]);
}
