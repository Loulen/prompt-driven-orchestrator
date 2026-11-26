//! The Projet of each Run, in memory (API 0.34): recorded when a Run starts,
//! before anyone hears of it, forgotten with the Run, loaded from the Résumés
//! at boot.

use pdo_kernel::Actor;
use pdo_run::application::ports::EventStore;
use pdo_run::domain::event::{Event, EventKind};

use crate::fakes::*;

fn run_started_in(run_id: &str, project_id: &str) -> Event {
    let mut started = run_started(run_id);
    started.payload.as_mut().unwrap()["project_id"] = serde_json::json!(project_id);
    started
}

#[tokio::test]
async fn a_run_answers_the_projet_it_was_launched_in() {
    let w = world();
    let append = w.runs.append_event();
    append.execute(&run_started_in("r1", "p1")).await.unwrap();
    append.execute(&run_started("r2")).await.unwrap();

    assert_eq!(w.runs.run_project("r1").as_deref(), Some("p1"));
    assert_eq!(w.runs.run_project("r2"), None, "a Run in no Projet");
    assert_eq!(w.runs.run_project("unknown"), None);
}

#[tokio::test]
async fn the_projet_is_known_before_the_bus_announces_the_run() {
    let w = world();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(None));
    let (runs, at_publish) = (std::sync::Arc::downgrade(&w.runs), seen.clone());
    *w.bus.probe.lock().unwrap() = Some(Box::new(move |_| {
        *at_publish.lock().unwrap() = runs.upgrade().unwrap().run_project("r1");
    }));

    w.runs
        .append_event()
        .execute(&run_started_in("r1", "p1"))
        .await
        .unwrap();

    assert_eq!(seen.lock().unwrap().as_deref(), Some("p1"));
}

#[tokio::test]
async fn a_refused_append_records_no_projet() {
    let w = world();
    w.store.forgotten.lock().unwrap().insert("gone".into());
    let _ = w
        .runs
        .append_event()
        .execute(&run_started_in("gone", "p1"))
        .await;
    assert_eq!(w.runs.run_project("gone"), None);
}

#[tokio::test]
async fn an_archived_run_keeps_its_projet_until_it_is_forgotten() {
    let w = world();
    let append = w.runs.append_event();
    append.execute(&run_started_in("r1", "p1")).await.unwrap();
    for kind in [EventKind::RunCompleted, EventKind::RunArchived] {
        append.execute(&event("r1", kind, None)).await.unwrap();
    }
    assert_eq!(w.runs.run_project("r1").as_deref(), Some("p1"));

    let forgotten = w.runs.forget_run(&Actor::LocalOperator, "r1").await;
    assert_eq!(forgotten.status(), 200);
    assert_eq!(w.runs.run_project("r1"), None);
}

#[tokio::test]
async fn the_boot_load_reads_the_projets_from_the_resumes() {
    let w = world();
    // Written to the store behind the index's back, as a previous daemon did.
    w.store.append(&run_started_in("r1", "p1")).await.unwrap();
    w.store.append(&run_started("r2")).await.unwrap();
    assert_eq!(w.runs.run_project("r1"), None);

    w.runs.load_run_projects().await;

    assert_eq!(w.runs.run_project("r1").as_deref(), Some("p1"));
    assert_eq!(w.runs.run_project("r2"), None);
}

#[tokio::test]
async fn a_failing_store_leaves_the_index_empty() {
    let w = world_over(Store {
        broken: true,
        ..Store::default()
    });
    w.runs.load_run_projects().await;
    assert_eq!(w.runs.run_project("r1"), None);
}
