//! The readiness sweep at a convergence fed by a bounded loop's guarded exit
//! (ADR-0082): the projection alone reads a FAIL lap not yet followed by its
//! next one as idle; the sweep holds the convergence off the reviewer's
//! frontmatter, and releases it once the loop exits.

use super::convergence_tests::{pipeline, state, verdict, DONE};
use super::*;
use crate::domain::state::{NodeStatus, RunState};

#[test]
fn the_sweep_holds_the_convergence_between_a_fail_lap_and_its_next_lap() {
    // The race a real daemon showed under load: the collection's barrier fired
    // and the reviewer's FAIL is in the log, but its routing has not appended
    // the next lap yet. Read off the projection alone, nothing is at work in
    // the loop; the reviewer's own back-edge says it goes on.
    let mut s = state(1, &[(1, DONE)], &[(1, DONE)]);
    s.collection_states.get_mut("topics_fanout").unwrap().done = true;
    let ready = |fm: &HashMap<String, HashMap<String, serde_yaml::Value>>| {
        let ready = dispatcher::compute_ready_to_spawn(&pipeline(), &s);
        assert!(
            ready.iter().any(|r| r.node_id == "summarizer"),
            "the projection alone reads the loop idle: {ready:?}"
        );
        dispatcher::hold_pending_loop_exits(&pipeline(), &s, ready, fm, &HashMap::new())
            .into_iter()
            .any(|r| r.node_id == "summarizer")
    };
    assert!(
        !ready(&verdict("FAIL")),
        "a FAIL lap whose back-edge fired resolves nothing"
    );
    assert!(
        ready(&verdict("PASS")),
        "a PASS lap fired the edge: the sweep may start summarizer"
    );
}

/// Whether the sweep, past its frontmatter-aware hold, starts summarizer on `s`
/// with the reviewer's latest lap reading `fm`.
fn swept(s: &RunState, fm: &HashMap<String, HashMap<String, serde_yaml::Value>>) -> bool {
    let ready = dispatcher::compute_ready_to_spawn(&pipeline(), s);
    dispatcher::hold_pending_loop_exits(&pipeline(), s, ready, fm, &HashMap::new())
        .into_iter()
        .any(|r| r.node_id == "summarizer")
}

/// The collection done, the loop at `lap` with both members settled on it.
fn settled_at(lap: i64) -> RunState {
    let laps: Vec<(i64, NodeStatus)> = (1..=lap).map(|i| (i, DONE)).collect();
    let mut s = state(lap, &laps, &laps);
    s.collection_states.get_mut("topics_fanout").unwrap().done = true;
    s
}

#[test]
fn the_sweep_releases_the_convergence_once_the_loop_exhausted() {
    // FAIL on the last lap: no later lap can fire the exit.
    assert!(swept(&settled_at(3), &verdict("FAIL")));
}

#[test]
fn the_sweep_releases_the_convergence_once_the_region_is_ended() {
    let mut s = settled_at(1);
    s.loop_states.get_mut("review_loop").unwrap().done = true;
    assert!(swept(&s, &verdict("FAIL")));
}

#[test]
fn the_sweep_releases_the_convergence_once_the_loop_is_force_routed() {
    // A force route replaces the lap's `when:` edges: no lap 2 starts, so the
    // FAIL's back-edge holds nothing — whether the region or the reviewer is
    // routed.
    for source in ["review_loop", "reviewer"] {
        let mut s = settled_at(1);
        s.forced_routes
            .insert(source.to_string(), "end".to_string());
        assert!(swept(&s, &verdict("FAIL")), "force route on {source}");
    }
}
