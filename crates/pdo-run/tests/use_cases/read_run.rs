//! `ReadRun`: a Run's events and its detail.

use pdo_run::domain::event::EventKind;
use pdo_run::domain::state::{NodeStatus, RunStatus};

use crate::fakes::*;

#[tokio::test]
async fn an_unknown_run_has_no_detail_and_no_events() {
    let w = world();
    assert!(w.runs.read_run().detail("nope").await.unwrap().is_none());
    assert!(w.runs.read_run().events("nope").await.unwrap().is_empty());
    assert!(w.disk.filled.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_log_without_run_started_has_no_detail() {
    let w = world();
    w.runs
        .append_event()
        .execute(&event("late", EventKind::RunPaused, None))
        .await
        .unwrap();
    assert!(w.runs.read_run().detail("late").await.unwrap().is_none());
}

#[tokio::test]
async fn the_detail_is_the_projection_filled_from_disk_with_the_manager_observed() {
    let w = world();
    let append = w.runs.append_event();
    append.execute(&run_started("r1")).await.unwrap();
    append
        .execute(&event("r1", EventKind::NodeStarted, Some("worker")))
        .await
        .unwrap();
    w.managers.0.lock().unwrap().insert("r1".into());

    let detail = w.runs.read_run().detail("r1").await.unwrap().unwrap();

    assert_eq!(detail.state.status, RunStatus::Running);
    assert_eq!(detail.state.nodes["worker"].status, NodeStatus::Running);
    assert_eq!(detail.state.loc.as_ref().unwrap().insertions, 3);
    assert_eq!(detail.events.len(), 2);
    assert!(detail.has_manager);
    assert_eq!(
        w.disk.filled.lock().unwrap().clone(),
        vec![("r1".to_string(), 2)]
    );
}

#[tokio::test]
async fn a_run_without_a_live_manager_says_so() {
    let w = world();
    w.runs
        .append_event()
        .execute(&run_started("r1"))
        .await
        .unwrap();
    let detail = w.runs.read_run().detail("r1").await.unwrap().unwrap();
    assert!(!detail.has_manager);
}

#[tokio::test]
async fn a_child_awaiting_its_user_lifts_its_running_parent_on_read() {
    let w = world();
    let append = w.runs.append_event();
    append.execute(&run_started("parent")).await.unwrap();
    append
        .execute(&event("parent", EventKind::NodeStarted, Some("worker")))
        .await
        .unwrap();
    let mut child = run_started("child");
    child.payload.as_mut().unwrap()["parent_run_id"] = serde_json::json!("parent");
    child.payload.as_mut().unwrap()["parent_node_id"] = serde_json::json!("worker");
    child.payload.as_mut().unwrap()["name"] = serde_json::json!("grill");
    append.execute(&child).await.unwrap();
    append
        .execute(&event("child", EventKind::NodeStarted, Some("worker")))
        .await
        .unwrap();
    let mut waits = event("child", EventKind::NodeAwaitingUser, Some("worker"));
    waits.payload = Some(serde_json::json!({ "cause": "declared", "message": "Which?" }));
    append.execute(&waits).await.unwrap();

    let parent = w.runs.read_run().detail("parent").await.unwrap().unwrap();

    assert_eq!(parent.state.status, RunStatus::AwaitingUser);
    assert_eq!(
        parent.state.awaiting_reason.as_deref(),
        Some("child run grill is awaiting you")
    );
    // Nothing was written: the parent's log is what it was.
    assert_eq!(parent.events.len(), 2);
}

#[tokio::test]
async fn a_log_that_cannot_be_read_is_an_error() {
    let w = world_over(Store {
        broken: true,
        ..Store::default()
    });
    assert!(w.runs.read_run().detail("r1").await.is_err());
    assert!(w.runs.read_run().events("r1").await.is_err());
}
