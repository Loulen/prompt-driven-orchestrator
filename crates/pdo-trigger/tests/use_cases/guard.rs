use std::path::PathBuf;
use std::time::Duration;

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use pdo_trigger::application::test_guard::GuardTestRequest;
use pdo_trigger::application::TriggerError;
use pdo_trigger::domain::fire_decision::GuardResult;

use crate::fakes::{world, REPO, TARGET_REPO_REQUIRED};

fn req(command: &str, repo: Option<&str>) -> GuardTestRequest {
    GuardTestRequest {
        guard_command: command.to_string(),
        target_repo: repo.map(str::to_string),
        project_id: None,
    }
}

#[tokio::test]
async fn the_dry_run_runs_the_guard_in_the_repo_with_the_fire_timeout_and_nothing_else() {
    let mut w = world();
    let result = w
        .triggers
        .test_guard(&Actor::LocalOperator, req("  echo hi ", Some(REPO)))
        .await
        .unwrap();
    assert_eq!(
        result,
        GuardResult::Pass {
            stdout: "from the guard".into(),
            stderr: String::new(),
        }
    );
    assert_eq!(
        w.guard.ran.lock().unwrap().clone(),
        vec![(
            "echo hi".to_string(),
            PathBuf::from(REPO),
            Duration::from_secs(7)
        )]
    );
    // Zero side effects: no Trigger, no fire, no Run, no announcement.
    assert!(w.store.all().is_empty());
    assert!(w.store.fires.lock().unwrap().is_empty());
    assert!(w.runs.started.lock().unwrap().is_empty());
    assert!(w.announced().is_empty());
    assert_eq!(
        w.host.asked.lock().unwrap().clone(),
        vec![(
            Actor::LocalOperator,
            Operation::new(ResourceKind::Trigger, Verb::Action("test_guard".into()))
        )]
    );
}

#[tokio::test]
async fn an_empty_command_or_an_unnamed_repo_never_runs() {
    let w = world();
    assert_eq!(
        w.triggers
            .test_guard(&Actor::LocalOperator, req("   ", Some(REPO)))
            .await
            .unwrap_err(),
        TriggerError::Invalid("guard_command must not be empty".into())
    );
    // The dry-run runs an arbitrary `sh -c`: like a real fire, never in a
    // repository nobody named.
    assert_eq!(
        w.triggers
            .test_guard(&Actor::LocalOperator, req("echo hi", None))
            .await
            .unwrap_err(),
        TriggerError::Invalid(TARGET_REPO_REQUIRED.into())
    );
    assert!(matches!(
        w.triggers
            .test_guard(&Actor::LocalOperator, req("echo hi", Some("/tmp/nowhere")))
            .await
            .unwrap_err(),
        TriggerError::Invalid(_)
    ));
    assert!(w.guard.ran.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_refused_dry_run_runs_nothing() {
    let w = world();
    w.refuse();
    assert!(matches!(
        w.triggers
            .test_guard(&Actor::LocalOperator, req("echo hi", Some(REPO)))
            .await
            .unwrap_err(),
        TriggerError::Refused(_)
    ));
    assert!(w.guard.ran.lock().unwrap().is_empty());
}
