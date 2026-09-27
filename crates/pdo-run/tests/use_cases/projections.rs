//! Every Run, projected and overlaid.

use pdo_run::application::projections::project_all_overlaid;
use pdo_run::domain::event::EventKind;

use crate::fakes::*;

#[tokio::test]
async fn every_started_run_is_projected_newest_first() {
    let w = world();
    let append = w.runs.append_event();
    append.execute(&run_started("20260101-a")).await.unwrap();
    append.execute(&run_started("20260201-b")).await.unwrap();
    // A fragment with no `RunStarted` is not a Run.
    append
        .execute(&event("20260301-c", EventKind::RunPaused, None))
        .await
        .unwrap();

    let runs = project_all_overlaid(w.store.as_ref()).await;

    let ids: Vec<&str> = runs.iter().map(|r| r.run_id.as_str()).collect();
    assert_eq!(ids, vec!["20260201-b", "20260101-a"]);
}

#[tokio::test]
async fn an_unreadable_log_projects_nothing() {
    let w = world_over(Store {
        broken: true,
        ..Store::default()
    });
    w.store
        .events
        .lock()
        .unwrap()
        .push(run_started("20260101-a"));
    assert!(project_all_overlaid(w.store.as_ref()).await.is_empty());
}
