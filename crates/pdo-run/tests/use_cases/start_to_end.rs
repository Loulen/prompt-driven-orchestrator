//! A Run whose Start is wired to End reaches End as soon as it starts
//! (CORE-142): with nothing to launch, it ends at once — one `RunCompleted`,
//! no NodeRun — instead of staying `running` forever.

use pdo_kernel::Actor;
use pdo_run::application::commands::RunCommandRequest;
use pdo_run::application::complete_node::CompleteNode;
use pdo_run::application::scheduler::advance::advance_run;
use pdo_run::application::stop_node::StopNode;
use pdo_run::domain::event::EventKind;
use pdo_run::domain::state::RunStatus;

use crate::fakes::*;

/// `start → end`, nothing else.
const START_END: &str = r#"name: start-end
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: end, port: result }
"#;

/// [`START_END`] plus an on-demand node only the user may launch.
const START_END_ON_DEMAND: &str = r#"name: start-end-on-demand
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: review
    name: Review
    type: agent
    isolated_worktree: false
    on_demand: true
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: end, port: result }
"#;

/// `start → end` and `start → root`, `root` wired to nothing, the Run waiting
/// for its started nodes.
const START_END_AND_ROOT: &str = r#"name: start-end-root
version: "1.0"
wait_for_started_nodes: true
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: root
    name: Root
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: end, port: result }
  - source: { node: start, port: user_prompt }
    target: { node: root, port: task }
"#;

/// `start → worker → end`: the ordinary relay, no `Start → End` edge.
const RELAY_TO_END: &str = r#"name: relay-to-end
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: Worker
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: out }
    target: { node: end, port: result }
"#;

fn completions(w: &World, run_id: &str) -> usize {
    kinds(w, run_id)
        .iter()
        .filter(|k| **k == EventKind::RunCompleted)
        .count()
}

fn assert_completed_at_once(w: &World, run_id: &str) {
    let run = state(w, run_id);
    assert_eq!(run.status, RunStatus::Completed, "{:?}", kinds(w, run_id));
    assert_eq!(
        kinds(w, run_id),
        vec![EventKind::RunStarted, EventKind::RunCompleted]
    );
    assert!(run.nodes.is_empty(), "no NodeRun: {:?}", run.nodes.keys());
    assert!(
        w.tmux.opened.lock().unwrap().is_empty(),
        "no session opened"
    );
}

#[tokio::test]
async fn a_start_to_end_run_completes_at_once() {
    let w = world();
    seed_frozen(&w, "r1", START_END).await;
    advance_run(&w.runs, "r1").await;
    settle().await;
    assert_completed_at_once(&w, "r1");
}

#[tokio::test]
async fn a_start_to_end_run_of_a_legacy_run_started_completes_at_once() {
    let w = world();
    seed_run_of(&w, "r1", START_END).await;
    advance_run(&w.runs, "r1").await;
    settle().await;
    assert_completed_at_once(&w, "r1");
}

#[tokio::test]
async fn advancing_a_start_to_end_run_twice_emits_one_run_completed() {
    let w = world();
    seed_frozen(&w, "r1", START_END).await;
    advance_run(&w.runs, "r1").await;
    advance_run(&w.runs, "r1").await;
    settle().await;
    assert_eq!(completions(&w, "r1"), 1, "{:?}", kinds(&w, "r1"));
}

#[tokio::test]
async fn an_on_demand_node_never_launched_holds_back_no_start_to_end_run() {
    let w = world();
    seed_frozen(&w, "r1", START_END_ON_DEMAND).await;
    advance_run(&w.runs, "r1").await;
    settle().await;
    assert_completed_at_once(&w, "r1");
}

#[tokio::test]
async fn a_start_to_end_run_that_waits_for_its_root_ends_once_the_root_completes() {
    let w = world();
    seed_frozen(&w, "r1", START_END_AND_ROOT).await;
    advance_run(&w.runs, "r1").await;
    settle().await;
    assert_eq!(state(&w, "r1").status, RunStatus::Running);
    assert_eq!(completions(&w, "r1"), 0);

    let dir = worktree(&w, "r1").join(".pdo/artifacts/root/iter-1/out");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("output.md"), "root 1\n").unwrap();
    let answer = w
        .runs
        .complete_node(
            &Actor::LocalOperator,
            CompleteNode {
                run_id: "r1".into(),
                node_id: "root".into(),
                iter: 1,
                auto: false,
            },
        )
        .await;
    assert_eq!(answer.status(), 200);
    settle().await;

    assert_eq!(
        state(&w, "r1").status,
        RunStatus::Completed,
        "{:?}",
        kinds(&w, "r1")
    );
    assert_eq!(completions(&w, "r1"), 1);
}

/// The end evaluated at launch never doubles the end owned by a completion:
/// « mark done » sweeps the Run before it fires the node's edges, and both
/// would otherwise end the Run.
#[tokio::test]
async fn marking_the_last_node_done_ends_an_ordinary_run_once() {
    let w = world();
    seed_frozen(&w, "r1", RELAY_TO_END).await;
    advance_run(&w.runs, "r1").await;
    write_out(&w, "r1", "worker");
    let request: RunCommandRequest = serde_json::from_value(
        serde_json::json!({ "kind": "mark_node_done", "node_id": "worker", "iter": 1 }),
    )
    .unwrap();
    let answer = w
        .runs
        .run_command(&Actor::LocalOperator, "r1".into(), request)
        .await;
    assert_eq!(answer.status(), 200);
    settle().await;

    assert_eq!(state(&w, "r1").status, RunStatus::Completed);
    assert_eq!(completions(&w, "r1"), 1, "{:?}", kinds(&w, "r1"));
}

/// A node the Run launched beside `Start → End` that is stopped does not let
/// the Run complete around it: the Run stays open for its Retry.
#[tokio::test]
async fn a_stopped_root_keeps_a_start_to_end_run_open() {
    let w = world();
    seed_frozen(&w, "r1", START_END_AND_ROOT).await;
    advance_run(&w.runs, "r1").await;
    let answer = w
        .runs
        .stop_node(
            &Actor::LocalOperator,
            StopNode {
                run_id: "r1".into(),
                node_id: "root".into(),
            },
        )
        .await;
    assert_eq!(answer.status(), 200);
    settle().await;
    advance_run(&w.runs, "r1").await;
    settle().await;

    assert_ne!(
        state(&w, "r1").status,
        RunStatus::Completed,
        "{:?}",
        kinds(&w, "r1")
    );
    assert_eq!(completions(&w, "r1"), 0);
}
