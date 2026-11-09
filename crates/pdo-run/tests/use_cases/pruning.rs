//! A pruned node — settled because its branch was not taken — is dead for
//! routing (ADR-0082): the sweep that prunes it fires none of its edges, so a
//! Run parked « unrouted » stays parked instead of completing, and a
//! convergence reads only the branches that fired.

use pdo_kernel::Actor;
use pdo_run::application::commands::RunCommandRequest;
use pdo_run::application::complete_node::CompleteNode;
use pdo_run::application::scheduler::advance::advance_run;
use pdo_run::domain::event::EventKind;
use pdo_run::domain::state::{NodeStatus, RunStatus};

use crate::fakes::*;

/// `gen → okn (when verdict = pass) → End`: the only path to `End` is guarded.
const UNROUTED: &str = r#"name: unrouted-lab
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: gen
    name: Gen
    type: agent
    isolated_worktree: false
    outputs:
      - name: out
  - id: okn
    name: Okn
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
    target: { node: gen, port: task }
  - source: { node: gen, port: out }
    target: { node: okn, port: in }
    when:
      verdict:
        eq: pass
  - source: { node: okn, port: out }
    target: { node: end, port: result }
"#;

/// `gen → okn (pass) → join` and `gen → elsen (else) → e2 → join → End`.
const DEAD_BRANCH: &str = r#"name: dead2-lab
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: gen
    name: Gen
    type: agent
    isolated_worktree: false
    outputs:
      - name: out
  - id: okn
    name: Okn
    type: agent
    isolated_worktree: false
    outputs:
      - name: out
  - id: elsen
    name: Elsen
    type: agent
    isolated_worktree: false
    outputs:
      - name: out
  - id: e2
    name: E2
    type: agent
    isolated_worktree: false
    outputs:
      - name: out
  - id: join
    name: Join
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
    target: { node: gen, port: task }
  - source: { node: gen, port: out }
    target: { node: okn, port: in }
    when:
      verdict:
        eq: pass
  - source: { node: gen, port: out }
    target: { node: elsen, port: in }
    else: true
  - source: { node: okn, port: out }
    target: { node: join, port: a }
  - source: { node: elsen, port: out }
    target: { node: e2, port: in }
  - source: { node: e2, port: out }
    target: { node: join, port: c }
  - source: { node: join, port: out }
    target: { node: end, port: result }
"#;

/// A review loop of one lap whose exit goes through `done` (when PASS).
const EXHAUSTED: &str = r#"name: exhausted-lab
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: impl
    name: impl
    type: agent
    isolated_worktree: false
    outputs:
      - name: out
  - id: rev
    name: rev
    type: agent
    isolated_worktree: false
    outputs:
      - name: out
  - id: done
    name: done
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
    target: { node: impl, port: task }
  - source: { node: impl, port: out }
    target: { node: rev, port: code }
  - source: { node: rev, port: out }
    target: { node: impl, port: task }
    else: true
  - source: { node: rev, port: out }
    target: { node: done, port: in }
    when:
      verdict:
        eq: PASS
  - source: { node: done, port: out }
    target: { node: end, port: result }
loops:
  - id: review_loop
    kind: bounded
    members: [impl, rev]
    max_iter: 1
"#;

/// Complete `node_id` at iter 1 with an `out` whose frontmatter says
/// `verdict`, the way its agent would.
async fn complete_with(w: &World, run_id: &str, node_id: &str, verdict: &str) {
    let dir = worktree(w, run_id)
        .join(".pdo/artifacts")
        .join(node_id)
        .join("iter-1/out");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("output.md"),
        format!("---\nverdict: {verdict}\n---\n{node_id}\n"),
    )
    .unwrap();
    let answer = w
        .runs
        .complete_node(
            &Actor::LocalOperator,
            CompleteNode {
                run_id: run_id.into(),
                node_id: node_id.into(),
                iter: 1,
                auto: false,
            },
        )
        .await;
    assert_eq!(answer.status(), 200, "completion of {node_id} refused");
    settle().await;
}

/// Run the Pipeline Manager command `json` on `r1`; it must be accepted.
async fn command(w: &World, json: serde_json::Value) {
    let request: RunCommandRequest = serde_json::from_value(json).unwrap();
    let answer = w
        .runs
        .run_command(&Actor::LocalOperator, "r1".into(), request)
        .await;
    assert_eq!(answer.status(), 200, "command refused");
    settle().await;
}

#[tokio::test]
async fn a_run_unrouted_by_its_routing_stays_parked_once_the_untaken_node_is_pruned() {
    let w = world();
    seed_frozen(&w, "r1", UNROUTED).await;
    advance_run(&w.runs, "r1").await;

    complete_with(&w, "r1", "gen", "other").await;
    // A later tick (another completion, a resume) sweeps the Run again.
    advance_run(&w.runs, "r1").await;

    let run = state(&w, "r1");
    let log = kinds(&w, "r1");
    assert_eq!(run.status, RunStatus::AwaitingUser, "{log:?}");
    assert_eq!(run.awaiting_reason_code.as_deref(), Some("unrouted"));
    assert_eq!(run.nodes["okn"].status, NodeStatus::Skipped);
    assert!(run.is_pruned("okn"));
    assert!(
        !log.contains(&EventKind::RunCompleted),
        "an unrouted Run completed through its pruned node: {log:?}"
    );

    // « Force route » to End lifts the park and ends the Run.
    command(
        &w,
        serde_json::json!({ "kind": "force_route", "from": "gen", "target": "end" }),
    )
    .await;
    assert_eq!(state(&w, "r1").status, RunStatus::Completed);
}

#[tokio::test]
async fn an_operator_skip_of_a_node_never_launched_still_feeds_downstream() {
    let w = world();
    seed_frozen(&w, "r1", DEAD_BRANCH).await;
    advance_run(&w.runs, "r1").await;
    complete_with(&w, "r1", "gen", "other").await;
    assert_eq!(state(&w, "r1").nodes["elsen"].status, NodeStatus::Running);

    // `e2` is not launched yet: the operator skips it with an output.
    command(
        &w,
        serde_json::json!({
            "kind": "skip_node", "node_id": "e2", "overrides": { "out": "by hand" }
        }),
    )
    .await;

    let run = state(&w, "r1");
    assert_eq!(run.nodes["e2"].status, NodeStatus::Skipped);
    assert!(!run.is_pruned("e2"), "an operator skip is no prune");
    assert_eq!(
        run.nodes.get("join").map(|n| n.status.clone()),
        Some(NodeStatus::Running),
        "the skipped node fed the convergence: {:?}",
        kinds(&w, "r1")
    );
}

#[tokio::test]
async fn a_convergence_reads_only_the_branches_that_fired() {
    let w = world();
    seed_frozen(&w, "r1", DEAD_BRANCH).await;
    advance_run(&w.runs, "r1").await;

    complete_with(&w, "r1", "gen", "pass").await;
    let run = state(&w, "r1");
    assert!(
        run.is_pruned("elsen") && run.is_pruned("e2"),
        "{:?}",
        run.nodes
    );
    assert!(!run.nodes.contains_key("join"), "join waits for okn");

    complete_with(&w, "r1", "okn", "pass").await;
    let run = state(&w, "r1");
    assert_eq!(run.nodes["join"].status, NodeStatus::Running);

    let preamble =
        std::fs::read_to_string(worktree(&w, "r1").join(".pdo/prompts/join-iter-1.md")).unwrap();
    assert!(
        preamble.contains("- `c`: not taken — its upstream `e2` was skipped"),
        "{preamble}"
    );
    assert!(
        !preamble.contains("e2/iter-1/out/output.md"),
        "the convergence was pointed at the untaken branch's output: {preamble}"
    );

    complete_with(&w, "r1", "join", "done").await;
    assert_eq!(state(&w, "r1").status, RunStatus::Completed);
}

#[tokio::test]
async fn an_exhausted_loop_whose_exit_branch_is_pruned_stays_parked() {
    let w = world();
    seed_frozen(&w, "r1", EXHAUSTED).await;
    advance_run(&w.runs, "r1").await;

    complete_with(&w, "r1", "impl", "done").await;
    complete_with(&w, "r1", "rev", "FAIL").await;
    advance_run(&w.runs, "r1").await;

    let run = state(&w, "r1");
    let log = kinds(&w, "r1");
    assert_eq!(run.status, RunStatus::AwaitingUser, "{log:?}");
    assert_eq!(
        run.awaiting_reason_code.as_deref(),
        Some("region_exhausted")
    );
    assert!(run.is_pruned("done"), "{:?}", run.nodes.get("done"));
    assert!(
        !log.contains(&EventKind::RunCompleted),
        "an exhausted loop completed the Run through its pruned exit: {log:?}"
    );
}
