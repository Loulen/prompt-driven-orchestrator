//! A convergence fed by a bounded loop's guarded exit, a collection's barrier
//! and a branch not taken — the dataflow of HP-02 (CORE-240). ADR-0082: the
//! convergence is ready once EVERY incoming edge is resolved; the FAIL of an
//! intermediate lap resolves nothing, the collection's barrier resolves only its
//! own edges.

use super::*;
use crate::domain::graph_resolver::ready_nodes;
use crate::domain::state::{
    CollectionState, IterationInfo, LoopState, NodeState, NodeStatus, RunState,
};

/// planner fans out to the review loop (coder ⇄ reviewer, exit `review` on
/// PASS) and the collection `itemizer`; `fallback` takes the `else`. All three
/// converge on `summarizer`.
const HP02_DATAFLOW: &str = r#"
name: hp02-dataflow
nodes:
  - { id: start, name: Start, type: start, outputs: [{ name: user_prompt }] }
  - { id: planner, name: planner, type: agent, outputs: [{ name: plan }] }
  - { id: coder, name: coder, type: agent, outputs: [{ name: code }] }
  - { id: reviewer, name: reviewer, type: agent, outputs: [{ name: review }] }
  - { id: itemizer, name: itemizer, type: agent, outputs: [{ name: item_note }] }
  - { id: fallback, name: fallback, type: agent, outputs: [{ name: note }] }
  - { id: summarizer, name: summarizer, type: agent, outputs: [{ name: summary }] }
  - { id: end, name: End, type: end, inputs: [{ name: result }] }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: planner, port: task }
  - source: { node: planner, port: plan }
    target: { node: coder, port: plan }
    when: { route: { eq: go } }
  - source: { node: planner, port: plan }
    target: { node: itemizer, port: plan }
    when: { route: { eq: go } }
  - source: { node: planner, port: plan }
    target: { node: fallback, port: plan }
    else: true
  - source: { node: coder, port: code }
    target: { node: reviewer, port: code }
  - source: { node: reviewer, port: review }
    target: { node: coder, port: review }
    when: { verdict: { eq: FAIL } }
  - source: { node: reviewer, port: review }
    target: { node: summarizer, port: review }
    when: { verdict: { eq: PASS } }
  - source: { node: itemizer, port: item_note }
    target: { node: summarizer, port: notes }
  - source: { node: fallback, port: note }
    target: { node: summarizer, port: notes }
  - source: { node: summarizer, port: summary }
    target: { node: end, port: result }
loops:
  - { id: review_loop, kind: bounded, members: [coder, reviewer], max_iter: 3 }
  - { id: topics_fanout, kind: collection, over: topics, members: [itemizer] }
"#;

pub(super) fn pipeline() -> PipelineDef {
    pdo_pipeline::domain::pipeline::parse_pipeline(HP02_DATAFLOW)
        .expect("fixture parses")
        .pipeline
}

pub(super) fn node(id: &str, laps: &[(i64, NodeStatus)]) -> NodeState {
    let (iter, status) = laps.last().cloned().expect("at least one lap");
    NodeState {
        missing_skills: Vec::new(),
        skipped_skills: Vec::new(),
        skills: None,
        isolated_worktree: None,
        harness: None,
        cost: None,
        node_id: id.into(),
        status,
        iter,
        started_at: Some("t0".into()),
        completed_at: None,
        failure_reason: None,
        skip_reason: None,
        pruned: false,
        iterations: laps
            .iter()
            .map(|(iter, status)| IterationInfo {
                iter: *iter,
                status: status.clone(),
                started_at: Some("t0".into()),
                completed_at: None,
                interactive: false,
                completion_released: false,
                waited_ms: 0,
                open_wait_since: None,
            })
            .collect(),
        frontmatter_retries: 0,
        frontmatter_violations: Vec::new(),
        missing_outputs: Vec::new(),
        delivery: None,
        awaiting: None,
    }
}

pub(super) const DONE: NodeStatus = NodeStatus::Completed;
const RUNNING: NodeStatus = NodeStatus::Running;

/// The Run once planner routed `go`: `fallback` pruned, the collection's two
/// laps done (its barrier not fired yet), the review loop at `lap`.
pub(super) fn state(
    lap: i64,
    coder: &[(i64, NodeStatus)],
    reviewer: &[(i64, NodeStatus)],
) -> RunState {
    let mut s = RunState::new("run-1".into(), "hp02-dataflow".into());
    s.nodes
        .insert("planner".into(), node("planner", &[(1, DONE)]));
    let mut fallback = node("fallback", &[(1, NodeStatus::Skipped)]);
    fallback.pruned = true;
    s.nodes.insert("fallback".into(), fallback);
    s.nodes
        .insert("itemizer".into(), node("itemizer", &[(1, DONE), (2, DONE)]));
    s.nodes.insert("coder".into(), node("coder", coder));
    if !reviewer.is_empty() {
        s.nodes
            .insert("reviewer".into(), node("reviewer", reviewer));
    }
    s.loop_states.insert(
        "review_loop".into(),
        LoopState {
            loop_node_id: "review_loop".into(),
            current_iter: lap,
            max_iter: 3,
            break_received: false,
            done: false,
        },
    );
    s.collection_states.insert(
        "topics_fanout".into(),
        CollectionState {
            region_id: "topics_fanout".into(),
            total_items: 2,
            entry: "itemizer".into(),
            members: vec!["itemizer".into()],
            done: false,
        },
    );
    s
}

pub(super) fn verdict(v: &str) -> HashMap<String, HashMap<String, serde_yaml::Value>> {
    let mut planner = HashMap::new();
    planner.insert("route".to_string(), serde_yaml::Value::String("go".into()));
    let mut reviewer = HashMap::new();
    reviewer.insert("verdict".to_string(), serde_yaml::Value::String(v.into()));
    HashMap::from([
        ("planner".to_string(), planner),
        ("reviewer".to_string(), reviewer),
    ])
}

fn spawns(actions: &[SchedulerAction], id: &str) -> bool {
    actions
        .iter()
        .any(|a| matches!(a, SchedulerAction::Spawn { node_id, .. } if node_id == id))
}

fn barrier(
    s: &RunState,
    fm: &HashMap<String, HashMap<String, serde_yaml::Value>>,
) -> Vec<SchedulerAction> {
    let pipeline = pipeline();
    let region = pipeline
        .loops
        .iter()
        .find(|r| r.id == "topics_fanout")
        .unwrap()
        .clone();
    evaluate_collection_barrier(&pipeline, s, &region, fm, &HashMap::new())
}

#[test]
fn the_barrier_does_not_start_the_convergence_while_the_reviewer_runs() {
    // F2-1 as the journal showed it: `collection_done`, then at once
    // `node_started summarizer`, the reviewer still running.
    let s = state(1, &[(1, DONE)], &[(1, RUNNING)]);
    let actions = barrier(&s, &HashMap::new());
    assert!(
        actions.contains(&SchedulerAction::CollectionDone {
            region_id: "topics_fanout".into()
        }),
        "the barrier itself fires: {actions:?}"
    );
    assert!(
        !spawns(&actions, "summarizer"),
        "summarizer must wait for the reviewer: {actions:?}"
    );
}

#[test]
fn the_barrier_does_not_start_the_convergence_on_an_intermediate_fail() {
    // Lap 1 failed, lap 2 is under way: the `review` edge did not fire, but a
    // later lap can fire it — it is not resolved.
    let s = state(2, &[(1, DONE), (2, RUNNING)], &[(1, DONE)]);
    let actions = barrier(&s, &verdict("FAIL"));
    assert!(
        !spawns(&actions, "summarizer"),
        "a FAIL lap with the loop still iterating resolves nothing: {actions:?}"
    );
}

#[test]
fn the_fail_lap_holds_the_convergence_before_the_next_lap_is_projected() {
    // The reviewer's FAIL is settled but the next lap is not in the log yet:
    // its own back-edge says the loop goes on.
    let s = state(1, &[(1, DONE)], &[(1, DONE)]);
    let actions = barrier(&s, &verdict("FAIL"));
    assert!(!spawns(&actions, "summarizer"), "{actions:?}");
}

#[test]
fn the_barrier_starts_the_convergence_once_the_loop_passed() {
    let s = state(2, &[(1, DONE), (2, DONE)], &[(1, DONE), (2, DONE)]);
    let actions = barrier(&s, &verdict("PASS"));
    assert!(
        actions.contains(&SchedulerAction::Spawn {
            node_id: "summarizer".into(),
            iter: 1
        }),
        "every branch resolved: the barrier starts summarizer: {actions:?}"
    );
}

#[test]
fn the_pass_starts_the_convergence_once_the_collection_is_done() {
    let mut s = state(2, &[(1, DONE), (2, DONE)], &[(1, DONE), (2, DONE)]);
    s.collection_states.get_mut("topics_fanout").unwrap().done = true;
    let fm = verdict("PASS");
    let actions = evaluate_outgoing_edges_full(
        &pipeline(),
        &s,
        "reviewer",
        &HashMap::new(),
        fm.get("reviewer").unwrap(),
        &fm,
    );
    assert!(
        actions.contains(&SchedulerAction::Spawn {
            node_id: "summarizer".into(),
            iter: 1
        }),
        "{actions:?}"
    );
}

#[test]
fn the_pass_waits_for_a_collection_still_fanning_out() {
    let mut s = state(2, &[(1, DONE), (2, DONE)], &[(1, DONE), (2, DONE)]);
    s.nodes.insert(
        "itemizer".into(),
        node("itemizer", &[(1, DONE), (2, RUNNING)]),
    );
    let fm = verdict("PASS");
    let actions = evaluate_outgoing_edges_full(
        &pipeline(),
        &s,
        "reviewer",
        &HashMap::new(),
        fm.get("reviewer").unwrap(),
        &fm,
    );
    assert!(!spawns(&actions, "summarizer"), "{actions:?}");
}

#[test]
fn the_readiness_sweep_waits_for_the_loop_exit() {
    // The collection is done and `fallback` pruned: only the review loop is
    // left, and its lap 2 is running.
    let mut s = state(2, &[(1, DONE), (2, RUNNING)], &[(1, DONE)]);
    s.collection_states.get_mut("topics_fanout").unwrap().done = true;
    assert!(
        !ready_nodes(&pipeline(), &s).contains(&"summarizer".to_string()),
        "the sweep must not start summarizer past an undecided guarded edge"
    );

    // The loop exits on PASS at lap 2: nothing is undecided any more.
    let s2 = {
        let mut s2 = state(2, &[(1, DONE), (2, DONE)], &[(1, DONE), (2, DONE)]);
        s2.collection_states.get_mut("topics_fanout").unwrap().done = true;
        s2
    };
    assert!(ready_nodes(&pipeline(), &s2).contains(&"summarizer".to_string()));
}

#[test]
fn the_readiness_sweep_waits_for_a_reviewer_not_started_yet() {
    let mut s = state(1, &[(1, RUNNING)], &[]);
    s.collection_states.get_mut("topics_fanout").unwrap().done = true;
    assert!(!ready_nodes(&pipeline(), &s).contains(&"summarizer".to_string()));
}

#[test]
fn an_exhaustion_exit_into_the_convergence_waits_for_the_collection() {
    // The loop's exhaustion routes to summarizer (`iter >= 3`), while the
    // collection still fans out: summarizer is not started by the exhaustion.
    let mut pipeline = pipeline();
    let mut exit = pipeline
        .edges
        .iter()
        .find(|e| e.source.node == "reviewer" && e.target.node == "summarizer")
        .unwrap()
        .clone();
    exit.when = Some(serde_yaml::from_str("iter: { gte: 3 }").unwrap());
    pipeline.edges.push(exit);
    let mut s = state(
        3,
        &[(1, DONE), (2, DONE), (3, DONE)],
        &[(1, DONE), (2, DONE), (3, DONE)],
    );
    s.nodes.insert(
        "itemizer".into(),
        node("itemizer", &[(1, DONE), (2, RUNNING)]),
    );
    let fm = verdict("FAIL");
    let actions = evaluate_outgoing_edges_full(
        &pipeline,
        &s,
        "reviewer",
        &HashMap::new(),
        fm.get("reviewer").unwrap(),
        &fm,
    );
    assert!(!spawns(&actions, "summarizer"), "{actions:?}");

    // The collection's barrier then starts it: the exhausted loop is resolved.
    s.nodes
        .insert("itemizer".into(), node("itemizer", &[(1, DONE), (2, DONE)]));
    let region = pipeline
        .loops
        .iter()
        .find(|r| r.id == "topics_fanout")
        .unwrap()
        .clone();
    let actions = evaluate_collection_barrier(&pipeline, &s, &region, &fm, &HashMap::new());
    assert!(
        actions.contains(&SchedulerAction::Spawn {
            node_id: "summarizer".into(),
            iter: 1
        }),
        "{actions:?}"
    );
}

#[test]
fn an_ended_region_is_not_iterating_any_more() {
    // The last lap failed and `end_region` closed the loop: its PASS exit will
    // never fire, so a convergence it feeds no longer waits on it.
    let mut s = state(2, &[(1, DONE), (2, DONE)], &[(1, DONE), (2, DONE)]);
    s.loop_states.get_mut("review_loop").unwrap().done = true;
    let actions = barrier(&s, &verdict("FAIL"));
    assert!(spawns(&actions, "summarizer"), "{actions:?}");
}

#[test]
fn the_convergence_preamble_names_only_the_branch_not_taken() {
    // When summarizer starts (loop passed, barrier fired), `review` and
    // `notes` from itemizer are there; only `fallback` is absent, not taken.
    let mut s = state(2, &[(1, DONE), (2, DONE)], &[(1, DONE), (2, DONE)]);
    s.collection_states.get_mut("topics_fanout").unwrap().done = true;
    let absent = crate::domain::input_resolution::absent_sources(&pipeline(), &s, "summarizer");
    assert_eq!(
        absent,
        HashMap::from([(
            "fallback".to_string(),
            crate::domain::input_resolution::Absence::NotTaken
        )])
    );
}
