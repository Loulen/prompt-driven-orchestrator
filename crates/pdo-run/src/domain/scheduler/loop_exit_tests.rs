//! A bounded loop that exited is no longer in flight, even when one of its
//! members ran on some laps only (CORE-240 review).

use super::convergence_tests::{node, verdict, DONE};
use super::*;
use crate::domain::state::{LoopState, RunState};

/// A loop whose `fixer` runs only on a FAIL lap: after a PASS on lap 2 it stays
/// at lap 1 for good. `escalate` hangs off the `BLOCKED` exit, never taken.
const PARTIAL_LAP_LOOP: &str = r#"
name: partial-lap-loop
nodes:
  - { id: start, name: Start, type: start, outputs: [{ name: user_prompt }] }
  - { id: coder, name: coder, type: agent, outputs: [{ name: code }] }
  - { id: reviewer, name: reviewer, type: agent, outputs: [{ name: review }] }
  - { id: fixer, name: fixer, type: agent, outputs: [{ name: fix }] }
  - { id: summarizer, name: summarizer, type: agent, outputs: [{ name: summary }] }
  - { id: escalate, name: escalate, type: agent, outputs: [{ name: out }] }
  - { id: end, name: End, type: end, inputs: [{ name: result }] }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: coder, port: task }
  - source: { node: coder, port: code }
    target: { node: reviewer, port: code }
  - source: { node: reviewer, port: review }
    target: { node: fixer, port: review }
    when: { verdict: { eq: FAIL } }
  - source: { node: fixer, port: fix }
    target: { node: coder, port: fix }
  - source: { node: reviewer, port: review }
    target: { node: summarizer, port: review }
    when: { verdict: { eq: PASS } }
  - source: { node: reviewer, port: review }
    target: { node: escalate, port: review }
    when: { verdict: { eq: BLOCKED } }
  - source: { node: summarizer, port: summary }
    target: { node: end, port: result }
  - source: { node: escalate, port: out }
    target: { node: end, port: result }
loops:
  - { id: review_loop, kind: bounded, members: [coder, reviewer, fixer], max_iter: 3 }
"#;

#[test]
fn a_member_that_skipped_the_last_lap_does_not_keep_the_loop_in_flight() {
    let pipeline = pdo_pipeline::domain::pipeline::parse_pipeline(PARTIAL_LAP_LOOP)
        .unwrap()
        .pipeline;
    let mut s = RunState::new("run-1".into(), "partial-lap-loop".into());
    s.nodes
        .insert("coder".into(), node("coder", &[(1, DONE), (2, DONE)]));
    s.nodes
        .insert("reviewer".into(), node("reviewer", &[(1, DONE), (2, DONE)]));
    s.nodes.insert("fixer".into(), node("fixer", &[(1, DONE)]));
    s.loop_states.insert(
        "review_loop".into(),
        LoopState {
            loop_node_id: "review_loop".into(),
            current_iter: 2,
            max_iter: 3,
            break_received: false,
            done: false,
        },
    );
    let fm = verdict("PASS");

    let region = &pipeline.loops[0];
    assert!(!crate::domain::loop_region::bounded_region_in_flight(
        &pipeline, region, &s
    ));
    let skips: Vec<String> = unreachable_nodes(&pipeline, &s, &fm, &HashMap::new())
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(
        skips,
        vec!["escalate".to_string()],
        "the BLOCKED exit is dead"
    );
}
