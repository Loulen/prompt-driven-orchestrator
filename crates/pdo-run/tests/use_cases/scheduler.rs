//! The scheduler drives a Run as a transition: under an extension that
//! refuses everything, it still spawns, completes and ends the Run, and it
//! never asks the Autorisation d'opération (ADR-0081, D10).

use pdo_kernel::Actor;
use pdo_run::application::complete_node::CompleteNode;
use pdo_run::application::scheduler::advance::advance_run;
use pdo_run::domain::event::EventKind;

use crate::fakes::*;

#[tokio::test]
async fn the_scheduler_spawns_the_entry_node_without_asking() {
    let w = world();
    *w.host.refuse.lock().unwrap() = true;
    seed_relay(&w, "r1").await;

    advance_run(&w.runs, "r1").await;

    assert_eq!(
        kinds(&w, "r1"),
        vec![EventKind::RunStarted, EventKind::NodeStarted]
    );
    let opened = w.tmux.opened.lock().unwrap().clone();
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].0, "pdo-r1-worker-iter-1");
    assert!(
        w.host.asked.lock().unwrap().is_empty(),
        "a scheduler transition asked the authorization"
    );
}

#[tokio::test]
async fn a_refused_extension_blocks_no_run_the_scheduler_drives() {
    let w = world();
    seed_relay(&w, "r1").await;
    advance_run(&w.runs, "r1").await;

    // The agent's completion is its gesture: allowed here, asked once.
    write_out(&w, "r1", "worker");
    let completion = CompleteNode {
        run_id: "r1".into(),
        node_id: "worker".into(),
        iter: 1,
        auto: false,
    };
    let answer = w
        .runs
        .complete_node(&Actor::LocalOperator, completion)
        .await;
    assert_eq!(answer.status(), 200);

    // From here on the extension refuses everything: the detached tail still
    // spawns `second`, because a spawn is a transition.
    *w.host.refuse.lock().unwrap() = true;
    eventually(&w, "r1", "second spawns", |kinds| {
        kinds
            .iter()
            .filter(|k| **k == EventKind::NodeStarted)
            .count()
            == 2
    })
    .await;
    let asked = w.host.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 1, "only the completion was asked: {asked:?}");
}
