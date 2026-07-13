//! Canonical input resolution (#194 / #210).
//!
//! THE single rule deciding which upstream iteration a consumer NodeRun reads
//! its inputs from: **the latest iteration of the source node that
//! Completed**. Artifacts written by failed iterations stay on disk (forensic
//! value) but are never resolvable as inputs; a feeder that only ever ran at
//! iter 1 keeps serving its iter-1 artifact to consumers at any lap — it is
//! never dragged into a loop lap (#195/#199).
//!
//! Every production resolution path delegates here:
//! - `prompt_augmenter::resolve_input_paths` (spawn-time preamble),
//! - `node_primitives::resolve_inputs` (manual `start_node`).
//!
//! This is a pure module: projected run state in, resolved iters out.

use std::collections::HashMap;

use crate::event_log::RunState;
use crate::pipeline::PipelineDef;

/// The iteration of `source_node_id` whose artifacts a consumer should read.
///
/// Picks the highest iteration recorded as `Completed` for the source —
/// skipping failed/stopped iterations, whose artifacts are quarantined from
/// resolution. Falls back to the source's head `iter` when its head status is
/// `Completed` but no per-iteration history exists (legacy states), and to
/// `consumer_iter` when the source has no completed iteration at all (the
/// path then points where the artifact will appear, preserving the previous
/// positional behavior for overrides/injection flows).
pub fn source_iter(run_state: &RunState, source_node_id: &str, consumer_iter: i64) -> i64 {
    run_state
        .latest_completed_iter(source_node_id)
        .unwrap_or(consumer_iter)
}

/// Resolves, for one consumer node, the source iteration of every incoming
/// edge: a map `source node id -> iter to read`. One entry per distinct
/// source feeding `node_id`.
pub fn resolved_source_iters(
    pipeline: &PipelineDef,
    run_state: &RunState,
    node_id: &str,
    consumer_iter: i64,
) -> HashMap<String, i64> {
    pipeline
        .edges
        .iter()
        .filter(|e| e.target.node == node_id)
        .map(|e| {
            (
                e.source.node.clone(),
                source_iter(run_state, &e.source.node, consumer_iter),
            )
        })
        .collect()
}

/// For one consumer node, the set of completed source iterations of every
/// `repeated` incoming edge: `source node id -> completed iters (asc)` (#353).
///
/// The set-valued twin of [`resolved_source_iters`], for `repeated`/pooled
/// inputs that accumulate one artifact per completed source lap. Only
/// `repeated` edges are included (non-repeated inputs resolve via
/// [`resolved_source_iters`]). Keyed by source node id, consistent with that
/// map. Delegates to [`RunState::completed_iters`] — the single authority; the
/// disk is never scanned for iterations, only read at the blessed ones. A
/// source with no completed iteration maps to an empty `Vec` (the pool is
/// empty — never a raw `iter-*` glob, which cannot exclude a failed iter).
pub fn resolved_repeated_iters(
    pipeline: &PipelineDef,
    run_state: &RunState,
    node_id: &str,
) -> HashMap<String, Vec<i64>> {
    pipeline
        .edges
        .iter()
        .filter(|e| e.target.node == node_id && e.repeated)
        .map(|e| {
            (
                e.source.node.clone(),
                run_state.completed_iters(&e.source.node),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_log::{IterationInfo, NodeState, NodeStatus};

    fn node_with_iterations(id: &str, iters: &[(i64, NodeStatus)]) -> NodeState {
        let (head_iter, head_status) = iters.last().cloned().unwrap_or((1, NodeStatus::Pending));
        NodeState {
            node_id: id.to_string(),
            status: head_status,
            iter: head_iter,
            started_at: None,
            completed_at: None,
            failure_reason: None,
            iterations: iters
                .iter()
                .map(|(iter, status)| IterationInfo {
                    iter: *iter,
                    status: status.clone(),
                    started_at: None,
                    completed_at: None,
                })
                .collect(),
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
        }
    }

    fn state_with(nodes: Vec<NodeState>) -> RunState {
        let mut s = RunState::new("run-1".into(), "test".into());
        for n in nodes {
            s.nodes.insert(n.node_id.clone(), n);
        }
        s
    }

    #[test]
    fn failed_iteration_artifacts_are_excluded_from_resolution() {
        // Forensic run 9c8d123: the griller wrote plan/ at iter 1 then FAILED,
        // and completed at iter 2. The implementer must read iter-2, never the
        // failed iter-1 plan.
        let state = state_with(vec![node_with_iterations(
            "griller",
            &[(1, NodeStatus::Failed), (2, NodeStatus::Completed)],
        )]);
        assert_eq!(source_iter(&state, "griller", 1), 2);
    }

    #[test]
    fn external_feeder_keeps_serving_its_completed_iter_at_any_lap() {
        // A feeder outside the loop region completed once at iter 1. A member
        // consumer at lap 3 still reads the feeder's iter-1 artifact — the
        // feeder is never expected to have produced an iter-3 input (#195).
        let state = state_with(vec![node_with_iterations(
            "feeder",
            &[(1, NodeStatus::Completed)],
        )]);
        assert_eq!(source_iter(&state, "feeder", 3), 1);
    }

    #[test]
    fn consumer_and_producer_align_on_the_lap_in_nominal_flow() {
        // Inside a region, the producer completes lap 2 right before the
        // consumer spawns at lap 2: latest-completed resolution gives the
        // positional alignment the nominal flow always had.
        let state = state_with(vec![node_with_iterations(
            "impl",
            &[(1, NodeStatus::Completed), (2, NodeStatus::Completed)],
        )]);
        assert_eq!(source_iter(&state, "impl", 2), 2);
    }

    #[test]
    fn source_without_any_completed_iteration_falls_back_to_consumer_iter() {
        // Override/injection flows: the path points where the artifact will
        // appear (positional), since nothing has completed yet.
        let state = state_with(vec![node_with_iterations(
            "up",
            &[(1, NodeStatus::Running)],
        )]);
        assert_eq!(source_iter(&state, "up", 1), 1);
        assert_eq!(source_iter(&state, "absent", 2), 2);
    }

    #[test]
    fn resolved_source_iters_maps_every_incoming_edge_join() {
        // Loop-entry join (#194): griller completed iter 2 (after a failed
        // iter 1), implementer completed iter 1. The join consumer resolves
        // each source independently — no positional iter alignment, no stall.
        use crate::pipeline::{EdgeDef, EdgeEndpoint, PipelineDef};
        let pipeline = PipelineDef {
            name: "join".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![],
            edges: vec![
                EdgeDef {
                    source: EdgeEndpoint {
                        node: "griller".into(),
                        port: "agentic_test".into(),
                    },
                    target: EdgeEndpoint {
                        node: "tester".into(),
                        port: "test".into(),
                    },
                    ..Default::default()
                },
                EdgeDef {
                    source: EdgeEndpoint {
                        node: "impl".into(),
                        port: "out".into(),
                    },
                    target: EdgeEndpoint {
                        node: "tester".into(),
                        port: "code".into(),
                    },
                    ..Default::default()
                },
            ],
            loops: vec![],
            notes: Vec::new(),
            prompt_required: true,
        };
        let state = state_with(vec![
            node_with_iterations(
                "griller",
                &[(1, NodeStatus::Failed), (2, NodeStatus::Completed)],
            ),
            node_with_iterations("impl", &[(1, NodeStatus::Completed)]),
        ]);
        let resolved = resolved_source_iters(&pipeline, &state, "tester", 1);
        assert_eq!(resolved.get("griller"), Some(&2));
        assert_eq!(resolved.get("impl"), Some(&1));
    }

    #[test]
    fn resolved_repeated_iters_pools_only_completed_source_iters() {
        // #353: a `repeated` edge reviewer→impl. The reviewer failed iter 2, so
        // the pool is {1, 3} — the failed iter is quarantined, never globbed in.
        // A second, non-repeated edge is ignored (it resolves via source_iter).
        use crate::pipeline::{EdgeDef, EdgeEndpoint, PipelineDef};
        let pipeline = PipelineDef {
            name: "cycle".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![],
            edges: vec![
                EdgeDef {
                    source: EdgeEndpoint {
                        node: "reviewer".into(),
                        port: "review".into(),
                    },
                    target: EdgeEndpoint {
                        node: "impl".into(),
                        port: "reviews".into(),
                    },
                    repeated: true,
                    ..Default::default()
                },
                EdgeDef {
                    source: EdgeEndpoint {
                        node: "planner".into(),
                        port: "plan".into(),
                    },
                    target: EdgeEndpoint {
                        node: "impl".into(),
                        port: "plan".into(),
                    },
                    repeated: false,
                    ..Default::default()
                },
            ],
            loops: vec![],
            notes: Vec::new(),
            prompt_required: true,
        };
        let state = state_with(vec![
            node_with_iterations(
                "reviewer",
                &[
                    (1, NodeStatus::Completed),
                    (2, NodeStatus::Failed),
                    (3, NodeStatus::Completed),
                ],
            ),
            node_with_iterations("planner", &[(1, NodeStatus::Completed)]),
        ]);
        let resolved = resolved_repeated_iters(&pipeline, &state, "impl");
        assert_eq!(resolved.get("reviewer"), Some(&vec![1, 3]));
        assert_eq!(
            resolved.get("planner"),
            None,
            "non-repeated edges are excluded — they resolve via resolved_source_iters"
        );
    }

    #[test]
    fn resolved_repeated_iters_empty_pool_when_source_has_no_completed_iter() {
        // #353 D6: a repeated source that has not completed any iteration yet
        // maps to an empty Vec — the pool is empty, not a raw glob.
        use crate::pipeline::{EdgeDef, EdgeEndpoint, PipelineDef};
        let pipeline = PipelineDef {
            name: "cycle".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![],
            edges: vec![EdgeDef {
                source: EdgeEndpoint {
                    node: "reviewer".into(),
                    port: "review".into(),
                },
                target: EdgeEndpoint {
                    node: "impl".into(),
                    port: "reviews".into(),
                },
                repeated: true,
                ..Default::default()
            }],
            loops: vec![],
            notes: Vec::new(),
            prompt_required: true,
        };
        let state = state_with(vec![node_with_iterations(
            "reviewer",
            &[(1, NodeStatus::Running)],
        )]);
        let resolved = resolved_repeated_iters(&pipeline, &state, "impl");
        assert_eq!(resolved.get("reviewer"), Some(&Vec::<i64>::new()));
    }
}
