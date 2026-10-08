//! The nodes the user decides to launch (ADR-0086): an on-demand node is
//! neither launched nor skipped by the runtime and holds nothing downstream,
//! a skipped node runs again through Retry, and a Run that waits for its
//! started nodes ends only once they are done.

use pdo_kernel::Actor;
use pdo_pipeline::domain::pipeline::parse_pipeline;
use pdo_run::application::complete_node::CompleteNode;
use pdo_run::application::retry_node::RetryNode;
use pdo_run::application::scheduler::advance::advance_run;
use pdo_run::application::start_node::StartNode;
use pdo_run::domain::event::{edge_info_from_pipeline, node_def_from_pipeline, EventKind};
use pdo_run::domain::projection::project;
use pdo_run::domain::state::{NodeStatus, RunState, RunStatus};

use crate::fakes::*;

/// `start → worker → end`, plus the on-demand `review` fed by `worker` and
/// wired to nothing, and `branch`, fed by `worker` through an edge that never
/// fires at iteration 1, on to `end`.
const PIPELINE: &str = r#"name: on-demand
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
  - id: review
    name: Review
    type: agent
    isolated_worktree: false
    on_demand: true
    outputs:
      - name: out
  - id: branch
    name: Branch
    type: agent
    isolated_worktree: false
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
    target: { node: review, port: work }
  - source: { node: worker, port: out }
    target: { node: branch, port: work }
    when: "iter >= 5"
  - source: { node: worker, port: out }
    target: { node: end, port: result }
  - source: { node: branch, port: out }
    target: { node: end, port: result }
"#;

/// A started Run of `yaml`, its `RunStarted` carrying the frozen graph the
/// way the create path writes it.
async fn seed(w: &World, run_id: &str, yaml: &str) {
    let repo = w.repo.path();
    let run_dir = repo.join(".pdo").join("runs").join(run_id);
    std::fs::create_dir_all(run_dir.join("worktree").join(".pdo").join("artifacts")).unwrap();
    std::fs::write(run_dir.join("pipeline.yaml"), yaml).unwrap();
    let prompts = run_dir.join("pipeline.prompts");
    std::fs::create_dir_all(&prompts).unwrap();
    let pipeline = parse_pipeline(yaml).unwrap().pipeline;
    for node in &pipeline.nodes {
        std::fs::write(prompts.join(format!("{}.md", node.id)), "Do it.\n").unwrap();
    }
    let node_defs: Vec<_> = pipeline.nodes.iter().map(node_def_from_pipeline).collect();
    let edges: Vec<_> = pipeline.edges.iter().map(edge_info_from_pipeline).collect();
    let mut started = event(run_id, EventKind::RunStarted, None);
    started.payload = Some(serde_json::json!({
        "pipeline_name": pipeline.name,
        "input": "go",
        "target_repo": repo.to_string_lossy(),
        "node_defs": node_defs,
        "edges": edges,
    }));
    if pipeline.wait_for_started_nodes {
        started.payload.as_mut().unwrap()["wait_for_started_nodes"] = serde_json::json!(true);
    }
    w.runs.append_event().execute(&started).await.unwrap();
}

fn state(w: &World, run_id: &str) -> RunState {
    let events: Vec<_> = w
        .store
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|e| e.run_id == run_id)
        .cloned()
        .collect();
    project(&events).unwrap()
}

fn status_of(w: &World, run_id: &str, node_id: &str) -> Option<NodeStatus> {
    state(w, run_id)
        .nodes
        .get(node_id)
        .map(|n| n.status.clone())
}

fn worktree(w: &World, run_id: &str) -> std::path::PathBuf {
    w.repo
        .path()
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree")
}

/// The preamble `node_id` was spawned with at `iter`.
fn prompt_of(w: &World, run_id: &str, node_id: &str, iter: i64) -> String {
    std::fs::read_to_string(
        worktree(w, run_id)
            .join(".pdo/prompts")
            .join(format!("{node_id}-iter-{iter}.md")),
    )
    .unwrap()
}

/// Complete `node_id` at `iter` the way its agent would, and wait for the
/// detached tail to have run.
async fn complete(w: &World, run_id: &str, node_id: &str, iter: i64) {
    let dir = worktree(w, run_id)
        .join(".pdo/artifacts")
        .join(node_id)
        .join(format!("iter-{iter}/out"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("output.md"), format!("{node_id} {iter}\n")).unwrap();
    let answer = w
        .runs
        .complete_node(
            &Actor::LocalOperator,
            CompleteNode {
                run_id: run_id.into(),
                node_id: node_id.into(),
                iter,
                auto: false,
            },
        )
        .await;
    assert_eq!(answer.status(), 200, "completion of {node_id} refused");
    settle().await;
}

/// Let the detached completion tails run to the end.
async fn settle() {
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
}

async fn start(w: &World, run_id: &str, node_id: &str) -> u16 {
    let answer = w
        .runs
        .start_node(
            &Actor::LocalOperator,
            StartNode {
                run_id: run_id.into(),
                node_id: node_id.into(),
            },
        )
        .await;
    let status = answer.status().as_u16();
    settle().await;
    status
}

async fn retry(w: &World, run_id: &str, node_id: &str) -> u16 {
    let answer = w
        .runs
        .retry_node(
            &Actor::LocalOperator,
            RetryNode {
                run_id: run_id.into(),
                node_id: node_id.into(),
            },
        )
        .await;
    let status = answer.status().as_u16();
    settle().await;
    status
}

fn completions(w: &World, run_id: &str) -> usize {
    kinds(w, run_id)
        .iter()
        .filter(|k| **k == EventKind::RunCompleted)
        .count()
}

#[tokio::test]
async fn the_runtime_neither_launches_nor_skips_an_on_demand_node() {
    let w = world();
    seed(&w, "r1", PIPELINE).await;

    advance_run(&w.runs, "r1").await;
    assert_eq!(status_of(&w, "r1", "worker"), Some(NodeStatus::Running));
    assert_eq!(status_of(&w, "r1", "review"), None, "launched at start");

    complete(&w, "r1", "worker", 1).await;
    let run = state(&w, "r1");
    assert_eq!(run.status, RunStatus::Completed, "{:?}", kinds(&w, "r1"));
    assert!(
        !run.nodes.contains_key("review"),
        "the runtime launched or skipped the on-demand node: {:?}",
        run.nodes.get("review").map(|n| n.status.clone())
    );
}

#[tokio::test]
async fn an_on_demand_root_is_not_launched_and_its_dead_upstream_does_not_skip_it() {
    let yaml = r#"name: roots
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
    outputs:
      - name: out
  - id: lonely
    name: Lonely
    type: agent
    isolated_worktree: false
    on_demand: true
  - id: gated
    name: Gated
    type: agent
    isolated_worktree: false
    on_demand: true
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: out }
    target: { node: gated, port: work }
    when: "iter >= 5"
  - source: { node: worker, port: out }
    target: { node: end, port: result }
"#;
    let w = world();
    seed(&w, "r1", yaml).await;
    advance_run(&w.runs, "r1").await;
    assert_eq!(status_of(&w, "r1", "lonely"), None, "a root on demand ran");

    complete(&w, "r1", "worker", 1).await;
    assert_eq!(state(&w, "r1").status, RunStatus::Completed);
    assert_eq!(
        status_of(&w, "r1", "gated"),
        None,
        "its dead upstream skipped it"
    );
}

/// `worker → join ← review`, `review → follow`, `join → end`.
const DOWNSTREAM: &str = r#"name: downstream
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
    outputs:
      - name: out
  - id: review
    name: Review
    type: agent
    isolated_worktree: false
    on_demand: true
    outputs:
      - name: out
  - id: join
    name: Join
    type: agent
    isolated_worktree: false
    outputs:
      - name: out
  - id: follow
    name: Follow
    type: agent
    isolated_worktree: false
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
    target: { node: join, port: work }
  - source: { node: review, port: out }
    target: { node: join, port: review }
  - source: { node: review, port: out }
    target: { node: follow, port: review }
  - source: { node: join, port: out }
    target: { node: end, port: result }
"#;

#[tokio::test]
async fn a_downstream_node_is_not_held_by_an_on_demand_node_never_launched() {
    let w = world();
    seed(&w, "r1", DOWNSTREAM).await;
    advance_run(&w.runs, "r1").await;
    assert_eq!(
        status_of(&w, "r1", "follow"),
        None,
        "fed by the on-demand node alone, it is no root"
    );

    complete(&w, "r1", "worker", 1).await;
    assert_eq!(
        status_of(&w, "r1", "join"),
        Some(NodeStatus::Running),
        "held by the on-demand node: {:?}",
        kinds(&w, "r1")
    );
    assert_eq!(status_of(&w, "r1", "follow"), None, "skipped or launched");
}

#[tokio::test]
async fn an_on_demand_node_serves_the_downstream_not_launched_yet_and_never_relaunches_a_settled_one(
) {
    let w = world();
    seed(&w, "r1", DOWNSTREAM).await;
    advance_run(&w.runs, "r1").await;
    complete(&w, "r1", "worker", 1).await;
    complete(&w, "r1", "join", 1).await;
    assert_eq!(state(&w, "r1").status, RunStatus::Completed);

    // The ▶ re-opens the Run and launches the node at iteration 1.
    assert_eq!(start(&w, "r1", "review").await, 200);
    let run = state(&w, "r1");
    assert_eq!(run.status, RunStatus::Running, "{:?}", kinds(&w, "r1"));
    assert_eq!(run.nodes["review"].iter, 1);
    assert_eq!(
        run.waiting_for_started_nodes, None,
        "without the option, a re-opened Run simply runs"
    );

    complete(&w, "r1", "review", 1).await;
    let run = state(&w, "r1");
    assert_eq!(
        run.nodes["follow"].status,
        NodeStatus::Running,
        "the downstream not launched yet was not served"
    );
    assert_eq!(
        run.nodes["join"].iter, 1,
        "the settled downstream ran again"
    );
    assert_eq!(run.nodes["join"].status, NodeStatus::Completed);

    // The Run ends again once the node it launched is done.
    complete(&w, "r1", "follow", 1).await;
    let run = state(&w, "r1");
    assert_eq!(run.status, RunStatus::Completed, "{:?}", kinds(&w, "r1"));
    assert_eq!(run.waiting_for_started_nodes, None);
    assert_eq!(completions(&w, "r1"), 2);
}

#[tokio::test]
async fn an_on_demand_node_reads_the_last_completed_iteration_and_names_an_absent_input() {
    let w = world();
    seed(&w, "r1", PIPELINE).await;
    advance_run(&w.runs, "r1").await;

    // Launched before its upstream produced anything: the input is absent.
    assert_eq!(start(&w, "r1", "review").await, 200);
    let preamble = prompt_of(&w, "r1", "review", 1);
    assert!(
        preamble.contains("`work`: not available yet") && preamble.contains("`worker`"),
        "{preamble}"
    );

    complete(&w, "r1", "review", 1).await;
    complete(&w, "r1", "worker", 1).await;

    // Launched again: Retry semantics, reading the upstream's last completed
    // iteration.
    assert_eq!(retry(&w, "r1", "review").await, 200);
    let preamble = prompt_of(&w, "r1", "review", 2);
    assert!(
        preamble.contains("- `work`: read `") && preamble.contains("worker/iter-1/out/output.md"),
        "{preamble}"
    );
}

#[tokio::test]
async fn retrying_a_skipped_node_runs_its_branch_as_if_taken() {
    let w = world();
    seed(&w, "r1", PIPELINE).await;
    advance_run(&w.runs, "r1").await;
    complete(&w, "r1", "worker", 1).await;
    // The Run ends with the branch it did not take skipped, not pending.
    assert_eq!(state(&w, "r1").status, RunStatus::Completed);
    assert_eq!(
        status_of(&w, "r1", "branch"),
        Some(NodeStatus::Skipped),
        "{:?}",
        kinds(&w, "r1")
    );
    assert_eq!(
        retry(&w, "r1", "branch").await,
        200,
        "{:?}",
        kinds(&w, "r1")
    );
    let run = state(&w, "r1");
    assert_eq!(run.status, RunStatus::Running);
    assert_eq!(run.nodes["branch"].status, NodeStatus::Running);

    assert_eq!(run.nodes["branch"].iter, 2, "not the next iteration");
    complete(&w, "r1", "branch", 2).await;
    assert_eq!(
        state(&w, "r1").status,
        RunStatus::Completed,
        "{:?}",
        kinds(&w, "r1")
    );
}

/// [`PIPELINE`] with « Attente des nœuds lancés ».
fn waiting_pipeline() -> String {
    PIPELINE.replacen(
        "version: \"1.0\"\n",
        "version: \"1.0\"\nwait_for_started_nodes: true\n",
        1,
    )
}

#[tokio::test]
async fn a_run_that_waits_for_its_started_nodes_stays_running_at_end_until_they_are_done() {
    let w = world();
    seed(&w, "r1", &waiting_pipeline()).await;
    advance_run(&w.runs, "r1").await;
    assert_eq!(start(&w, "r1", "review").await, 200);

    complete(&w, "r1", "worker", 1).await;
    let run = state(&w, "r1");
    assert_eq!(run.status, RunStatus::Running, "{:?}", kinds(&w, "r1"));
    assert_eq!(run.waiting_for_started_nodes, Some(1));
    let summary = w
        .store
        .summaries
        .lock()
        .unwrap()
        .get("r1")
        .cloned()
        .unwrap();
    assert_eq!(summary.status, RunStatus::Running);
    assert_eq!(summary.waiting_for_started_nodes, Some(1));

    complete(&w, "r1", "review", 1).await;
    let run = state(&w, "r1");
    assert_eq!(run.status, RunStatus::Completed, "{:?}", kinds(&w, "r1"));
    assert_eq!(run.waiting_for_started_nodes, None);
    assert_eq!(completions(&w, "r1"), 1);
    let summary = w
        .store
        .summaries
        .lock()
        .unwrap()
        .get("r1")
        .cloned()
        .unwrap();
    assert_eq!(summary.status, RunStatus::Completed);
}

#[tokio::test]
async fn without_the_option_a_run_ends_at_end_while_a_started_node_runs() {
    let w = world();
    seed(&w, "r1", PIPELINE).await;
    advance_run(&w.runs, "r1").await;
    assert_eq!(start(&w, "r1", "review").await, 200);

    complete(&w, "r1", "worker", 1).await;
    assert_eq!(state(&w, "r1").status, RunStatus::Completed);
    assert_eq!(completions(&w, "r1"), 1);
}

#[tokio::test]
async fn an_on_demand_node_never_launched_holds_no_waiting_run() {
    let w = world();
    seed(&w, "r1", &waiting_pipeline()).await;
    advance_run(&w.runs, "r1").await;
    complete(&w, "r1", "worker", 1).await;
    assert_eq!(state(&w, "r1").status, RunStatus::Completed);
}

/// The end evaluation replays the routing as it ran: a `force_route` to `End`
/// ends the Run once the started nodes are done, too.
#[tokio::test]
async fn a_forced_route_to_end_ends_the_run_once_the_started_nodes_are_done() {
    let w = world();
    seed(&w, "r1", &waiting_pipeline()).await;
    advance_run(&w.runs, "r1").await;
    assert_eq!(start(&w, "r1", "review").await, 200);
    let forced = w
        .runs
        .run_command(
            &Actor::LocalOperator,
            "r1".into(),
            serde_json::from_value(
                serde_json::json!({ "kind": "force_route", "from": "worker", "target": "end" }),
            )
            .unwrap(),
        )
        .await;
    assert_eq!(forced.status(), 200);

    complete(&w, "r1", "worker", 1).await;
    assert_eq!(
        state(&w, "r1").status,
        RunStatus::Running,
        "{:?}",
        kinds(&w, "r1")
    );

    complete(&w, "r1", "review", 1).await;
    assert_eq!(
        state(&w, "r1").status,
        RunStatus::Completed,
        "{:?}",
        kinds(&w, "r1")
    );
    assert_eq!(completions(&w, "r1"), 1);
}

/// An edge into `End` with a `reason:` halts the Run — after the started
/// nodes too, and as a halt, not a completion.
#[tokio::test]
async fn a_halt_at_end_waits_for_the_started_nodes_and_stays_a_halt() {
    let yaml = waiting_pipeline().replacen(
        "  - source: { node: worker, port: out }\n    target: { node: end, port: result }\n",
        "  - source: { node: worker, port: out }\n    target: { node: end, port: result }\n    reason: \"enough\"\n",
        1,
    );
    assert!(yaml.contains("reason: \"enough\""));
    let w = world();
    seed(&w, "r1", &yaml).await;
    advance_run(&w.runs, "r1").await;
    assert_eq!(start(&w, "r1", "review").await, 200);

    complete(&w, "r1", "worker", 1).await;
    assert_eq!(
        state(&w, "r1").status,
        RunStatus::Running,
        "{:?}",
        kinds(&w, "r1")
    );

    complete(&w, "r1", "review", 1).await;
    let run = state(&w, "r1");
    assert_eq!(run.status, RunStatus::Halted, "{:?}", kinds(&w, "r1"));
    assert_eq!(completions(&w, "r1"), 0);
}

/// Stopping the last started node of a Run past `End` ends it as well.
#[tokio::test]
async fn stopping_the_last_started_node_ends_a_waiting_run() {
    let w = world();
    seed(&w, "r1", &waiting_pipeline()).await;
    advance_run(&w.runs, "r1").await;
    assert_eq!(start(&w, "r1", "review").await, 200);
    complete(&w, "r1", "worker", 1).await;
    assert_eq!(state(&w, "r1").status, RunStatus::Running);

    let stopped = w
        .runs
        .stop_node(
            &Actor::LocalOperator,
            pdo_run::application::stop_node::StopNode {
                run_id: "r1".into(),
                node_id: "review".into(),
            },
        )
        .await;
    assert_eq!(stopped.status(), 200);
    settle().await;
    assert_eq!(
        state(&w, "r1").status,
        RunStatus::Completed,
        "{:?}",
        kinds(&w, "r1")
    );
}
