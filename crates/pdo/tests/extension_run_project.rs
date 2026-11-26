//! API 0.34 against the real daemon, with the test extension: the Projet a
//! Run was launched in, as the extension's `Host::run_project` reads it.
//!
//! It is the `project_id` `GET /runs/<id>` shows; `null` for a Run in no
//! Projet and an unknown Run; still there after a restart (loaded from the
//! Résumés before the daemon serves); gone once the Run is forgotten.

use pdo_run::domain::event::{Event, EventKind};
use serde_json::{json, Value};

use crate::common::TestDaemon;
use crate::test_extension::{Refusal, TestExtension};

const IN_A_PROJET: &str = "20261126-100000-in-projet";
const IN_NO_PROJET: &str = "20261126-100000-no-projet";
const PROJET: &str = "prj-20261126-100000-aaaaaaa";

fn extensions() -> Vec<Box<dyn pdo::extension_api::Extension>> {
    TestExtension::new(Refusal::Nothing).0.boxed()
}

async fn seed_runs(daemon: &TestDaemon, target_repo: &str) {
    daemon
        .seed_event(Event {
            id: None,
            run_id: IN_A_PROJET.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(json!({
                "pipeline_name": "solo",
                "target_repo": target_repo,
                "project_id": PROJET,
            })),
        })
        .await;
    daemon.seed_run_in(IN_NO_PROJET, "solo", target_repo).await;
}

/// What the extension reads of `run_id`'s Projet.
async fn run_project(daemon: &TestDaemon, run_id: &str) -> Value {
    let resp = reqwest::Client::new()
        .get(format!("{}/ext/test/runs/{run_id}/view", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    resp.json::<Value>().await.unwrap()["project"].clone()
}

#[tokio::test]
async fn the_host_reads_the_projet_get_runs_shows() {
    let daemon = TestDaemon::spawn_with_extensions(crate::common::flow::git_init, extensions())
        .await
        .unwrap();
    seed_runs(&daemon, &daemon.target_repo()).await;

    let shown = daemon.get_json(&format!("/runs/{IN_A_PROJET}")).await;
    assert_eq!(shown["project_id"], PROJET, "{shown}");
    assert_eq!(run_project(&daemon, IN_A_PROJET).await, PROJET);
    assert_eq!(run_project(&daemon, IN_NO_PROJET).await, Value::Null);
    assert_eq!(run_project(&daemon, "does-not-exist").await, Value::Null);
}

#[tokio::test]
async fn the_projet_survives_a_restart_and_goes_with_a_forgotten_run() {
    let root = tempfile::tempdir().unwrap();
    crate::common::flow::git_init(root.path()).unwrap();
    let first = TestDaemon::spawn_at_with_extensions(root.path(), extensions())
        .await
        .unwrap();
    seed_runs(&first, &root.path().to_string_lossy()).await;
    drop(first);

    let daemon = TestDaemon::spawn_at_with_extensions(root.path(), extensions())
        .await
        .unwrap();
    assert_eq!(run_project(&daemon, IN_A_PROJET).await, PROJET);
    assert_eq!(run_project(&daemon, IN_NO_PROJET).await, Value::Null);

    daemon.seed_run_archived(IN_A_PROJET).await;
    let forgotten = reqwest::Client::new()
        .delete(format!("{}/runs/{IN_A_PROJET}", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(forgotten.status(), 200);
    assert_eq!(run_project(&daemon, IN_A_PROJET).await, Value::Null);
}
