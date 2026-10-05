//! The reads of the Runs go through the Autorisation d'opération (CORE-5): a
//! list keeps the Runs the actor may read, one Run is refused, both in the
//! Projet of the Run's primary repository.

use axum::body::to_bytes;
use axum::response::Response;
use pdo_kernel::attributes::AttributeFilter;
use pdo_kernel::{Actor, ResourceKind, Verb};
use pdo_run::application::list::reapable::ReapableQuery;
use pdo_run::domain::event::EventKind;
use pdo_workspace::domain::project::Project;

use crate::fakes::*;

async fn json(response: Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

/// A Run started in `repo`, child of `parent` when given.
async fn seed_in(w: &World, run_id: &str, repo: &std::path::Path, parent: Option<&str>) {
    let mut started = event(run_id, EventKind::RunStarted, None);
    let mut payload = serde_json::json!({
        "pipeline_name": "relay",
        "input": "go",
        "target_repo": repo.to_string_lossy(),
    });
    if let Some(parent) = parent {
        payload["parent_run_id"] = parent.into();
        payload["parent_node_id"] = "worker".into();
    }
    started.payload = Some(payload);
    w.runs.append_event().execute(&started).await.unwrap();
}

/// Two Projets: `p1` owns the world's repository, `p2` another one. `r1` runs
/// in `p1`, `r2` in `p2`, `r3` (a child of `r1`) in `p2`, `r4` in no Projet.
async fn two_projets() -> (World, tempfile::TempDir, tempfile::TempDir) {
    let w = world();
    let other = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let mut p1 = Project::named("p1", "Alpha");
    p1.members = vec![w.repo.path().to_string_lossy().into_owned()];
    let mut p2 = Project::named("p2", "Beta");
    p2.members = vec![other.path().to_string_lossy().into_owned()];
    w.projects.0.lock().unwrap().extend([p1, p2]);
    seed_in(&w, "r1", w.repo.path(), None).await;
    seed_in(&w, "r2", other.path(), None).await;
    seed_in(&w, "r3", other.path(), Some("r1")).await;
    seed_in(&w, "r4", outside.path(), None).await;
    (w, other, outside)
}

fn ids(list: &serde_json::Value) -> Vec<String> {
    let mut ids: Vec<String> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|run| run["run_id"].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    ids
}

#[tokio::test]
async fn the_list_keeps_only_the_runs_the_actor_may_read() {
    let (w, _other, _outside) = two_projets().await;
    let actor = Actor::LocalOperator;
    let all = json(w.runs.list_runs(&actor, AttributeFilter::default()).await).await;
    assert_eq!(ids(&all), ["r1", "r2", "r3", "r4"]);

    *w.host.refuse_reads_in.lock().unwrap() = vec!["p2".into()];
    w.host.asked.lock().unwrap().clear();
    let kept = json(w.runs.list_runs(&actor, AttributeFilter::default()).await).await;
    assert_eq!(ids(&kept), ["r1", "r4"], "the Runs of p2 are hidden");

    let asked = w.host.asked.lock().unwrap().clone();
    let reads: Vec<_> = asked
        .iter()
        .map(|(_, op)| op)
        .filter(|op| op.resource == ResourceKind::Run && op.verb == Verb::Read)
        .collect();
    assert_eq!(reads.len(), 4, "one question per Run: {asked:?}");
    let r4 = reads
        .iter()
        .find(|op| op.target.as_deref() == Some("r4"))
        .unwrap();
    assert!(
        r4.is_outside_projects(),
        "a Run of no Projet is read hors Projet"
    );
    let r2 = reads
        .iter()
        .find(|op| op.target.as_deref() == Some("r2"))
        .unwrap();
    assert_eq!(r2.projects, ["p2"]);
}

#[tokio::test]
async fn one_run_is_refused_in_the_projet_of_its_repository() {
    let (w, _other, _outside) = two_projets().await;
    *w.host.refuse_reads_in.lock().unwrap() = vec!["p2".into()];
    let actor = Actor::LocalOperator;
    assert_eq!(w.runs.authorize_read(&actor, "r1").await, Ok(()));
    let denied = w.runs.authorize_read(&actor, "r2").await.unwrap_err();
    assert_eq!(denied.reason, "the test host refuses this read");
    assert!(w.runs.authorize_read(&actor, "r4").await.is_ok());
}

#[tokio::test]
async fn the_children_of_a_run_keep_only_the_readable_ones() {
    let (w, _other, _outside) = two_projets().await;
    let actor = Actor::LocalOperator;
    let children = json(w.runs.run_children(&actor, "r1").await).await;
    assert_eq!(children["nodes"][0]["children"][0]["run_id"], "r3");

    *w.host.refuse_reads_in.lock().unwrap() = vec!["p2".into()];
    let children = json(w.runs.run_children(&actor, "r1").await).await;
    assert_eq!(children["nodes"], serde_json::json!([]), "r3 is in p2");
}

#[tokio::test]
async fn the_reapable_runs_keep_only_the_readable_ones() {
    let (w, other, _outside) = two_projets().await;
    for (run_id, repo) in [("r1", w.repo.path()), ("r2", other.path())] {
        std::fs::create_dir_all(repo.join(".pdo/runs").join(run_id).join("worktree")).unwrap();
        let mut done = event(run_id, EventKind::RunCompleted, None);
        done.ts = "2026-09-27T12:00:01.000Z".into();
        w.runs.append_event().execute(&done).await.unwrap();
    }
    let actor = Actor::LocalOperator;
    let query: ReapableQuery = serde_json::from_value(serde_json::json!({})).unwrap();
    let all = json(w.runs.list_reapable(&actor, query).await).await;
    assert_eq!(ids(&all), ["r1", "r2"]);

    *w.host.refuse_reads_in.lock().unwrap() = vec!["p2".into()];
    let query: ReapableQuery = serde_json::from_value(serde_json::json!({})).unwrap();
    let kept = json(w.runs.list_reapable(&actor, query).await).await;
    assert_eq!(ids(&kept), ["r1"]);
}
