//! `AppendEvent`: the transition guard, then the store, then the bus.

use pdo_extension_api::RunNames;
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

/// A `RunStarted` named `name` (none when `None`), its `worker` named
/// "Assistant".
fn named_run_started(run_id: &str, name: Option<&str>) -> pdo_run::domain::event::Event {
    let mut started = run_started(run_id);
    let payload = started.payload.as_mut().unwrap();
    payload["node_defs"][0]["name"] = serde_json::json!("Assistant");
    if let Some(name) = name {
        payload["name"] = serde_json::json!(name);
    }
    started
}

fn renamed(run_id: &str, name: &str) -> pdo_run::domain::event::Event {
    let mut renamed = event(run_id, EventKind::RunRenamed, None);
    renamed.payload = Some(serde_json::json!({ "name": name }));
    renamed
}

#[tokio::test]
async fn a_started_run_is_named_with_its_named_nodes_before_it_is_announced() {
    let w = world();
    let mut frames = pdo_kernel::event_bus::EventBus::subscribe(w.bus.as_ref());
    assert_eq!(w.runs.run_names("r1"), None);
    w.runs
        .append_event()
        .execute(&named_run_started("r1", Some("Démo vendredi")))
        .await
        .unwrap();
    frames.recv().await.unwrap();
    assert_eq!(
        w.runs.run_names("r1"),
        Some(RunNames::new("Démo vendredi").with_node("worker", "Assistant"))
    );
    assert_eq!(w.runs.run_names("unknown"), None);
}

#[tokio::test]
async fn a_renaming_is_followed_and_an_empty_name_falls_back_to_the_pipeline() {
    let w = world();
    let append = w.runs.append_event();
    append
        .execute(&named_run_started("r1", Some("Démo")))
        .await
        .unwrap();
    append.execute(&renamed("r1", "Nouveau")).await.unwrap();
    assert_eq!(w.runs.run_names("r1").unwrap().name, "Nouveau");
    append.execute(&renamed("r1", "")).await.unwrap();
    assert_eq!(w.runs.run_names("r1").unwrap().name, "p");
}

#[tokio::test]
async fn an_unnamed_run_is_named_by_its_pipeline_and_an_archived_one_not_at_all() {
    let w = world();
    let append = w.runs.append_event();
    append
        .execute(&named_run_started("r1", None))
        .await
        .unwrap();
    assert_eq!(w.runs.run_names("r1").unwrap().name, "p");
    for kind in [EventKind::RunCompleted, EventKind::RunArchived] {
        append.execute(&event("r1", kind, None)).await.unwrap();
    }
    assert_eq!(w.runs.run_names("r1"), None);
}

#[tokio::test]
async fn the_names_are_loaded_from_the_summaries_at_boot() {
    let w = world();
    let append = w.runs.append_event();
    append
        .execute(&named_run_started("r1", Some("Démo")))
        .await
        .unwrap();
    append
        .execute(&named_run_started("r2", None))
        .await
        .unwrap();
    for kind in [EventKind::RunCompleted, EventKind::RunArchived] {
        append.execute(&event("r2", kind, None)).await.unwrap();
    }
    // Written by an older shape: rebuilt, then read into the names.
    w.store.summaries.lock().unwrap().clear();
    *w.store.summaries_current.lock().unwrap() = false;
    assert!(
        pdo_run::application::run_summary::ensure_summaries(w.store.as_ref())
            .await
            .unwrap()
    );
    w.store
        .summaries
        .lock()
        .unwrap()
        .get_mut("r1")
        .unwrap()
        .name = Some("Relu".into());
    w.runs.load_run_names().await.unwrap();
    assert_eq!(
        w.runs.run_names("r1"),
        Some(RunNames::new("Relu").with_node("worker", "Assistant"))
    );
    assert_eq!(w.runs.run_names("r2"), None, "archived");
}
