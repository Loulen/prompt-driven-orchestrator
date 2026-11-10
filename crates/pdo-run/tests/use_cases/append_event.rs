//! `AppendEvent`: the transition guard, then the store, then the bus.

use pdo_run::domain::event::EventKind;

use crate::fakes::*;

#[tokio::test]
async fn an_event_is_stored_then_published_as_the_ws_frame() {
    let w = world();
    let started = run_started("r1");
    w.runs.append_event().execute(&started).await.unwrap();

    let stored = w.store.events.lock().unwrap().clone();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].kind, EventKind::RunStarted);
    let published = w.bus.published.lock().unwrap().clone();
    assert_eq!(published, vec![started.bus_message()]);
    assert_eq!(published[0]["type"], "event");
    assert_eq!(published[0]["event"]["kind"], "run_started");
    assert_eq!(published[0]["event"]["run_id"], "r1");
}

#[tokio::test]
async fn a_legal_node_transition_goes_through() {
    let w = world();
    let append = w.runs.append_event();
    append.execute(&run_started("r1")).await.unwrap();
    append
        .execute(&event("r1", EventKind::NodeStarted, Some("worker")))
        .await
        .unwrap();
    append
        .execute(&event("r1", EventKind::NodeCompleted, Some("worker")))
        .await
        .unwrap();
    assert_eq!(w.store.events.lock().unwrap().len(), 3);
    assert_eq!(w.bus.published.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn a_duplicate_completion_is_a_no_op_that_stores_and_publishes_nothing() {
    let w = world();
    let append = w.runs.append_event();
    append.execute(&run_started("r1")).await.unwrap();
    append
        .execute(&event("r1", EventKind::NodeStarted, Some("worker")))
        .await
        .unwrap();
    let completed = event("r1", EventKind::NodeCompleted, Some("worker"));
    append.execute(&completed).await.unwrap();

    append.execute(&completed).await.unwrap();

    assert_eq!(w.store.events.lock().unwrap().len(), 3);
    assert_eq!(w.bus.published.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn an_illegal_transition_is_refused_before_the_store() {
    let w = world();
    let append = w.runs.append_event();
    append.execute(&run_started("r1")).await.unwrap();

    let error = append
        .execute(&event("r1", EventKind::NodeCompleted, Some("worker")))
        .await
        .unwrap_err();

    assert!(
        error
            .to_string()
            .starts_with("transition rejected (NodeCompleted): "),
        "{error}"
    );
    assert_eq!(w.store.events.lock().unwrap().len(), 1);
    assert_eq!(w.bus.published.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_forgotten_run_refuses_the_append_and_nothing_is_published() {
    let w = world();
    w.store.forgotten.lock().unwrap().insert("gone".into());

    let error = w
        .runs
        .append_event()
        .execute(&run_started("gone"))
        .await
        .unwrap_err();

    assert_eq!(error.to_string(), "run gone has been forgotten");
    assert!(w.bus.published.lock().unwrap().is_empty());
}

#[tokio::test]
async fn only_node_transitions_read_the_log_first() {
    let w = world_over(Store {
        broken: true,
        ..Store::default()
    });
    let append = w.runs.append_event();

    append
        .execute(&event("r1", EventKind::RunPaused, None))
        .await
        .unwrap();
    let error = append
        .execute(&event("r1", EventKind::NodeStarted, Some("worker")))
        .await
        .unwrap_err();

    assert_eq!(error.to_string(), "failed to load events");
    assert_eq!(w.store.events.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn the_bus_subscribers_receive_the_published_frame() {
    let w = world();
    let mut frames = pdo_kernel::event_bus::EventBus::subscribe(w.bus.as_ref());
    let started = run_started("r1");
    w.runs.append_event().execute(&started).await.unwrap();
    assert_eq!(frames.recv().await.unwrap(), started.bus_message());
}
