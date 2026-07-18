//! Canonical input resolution (#194 / #210 / #353 / #370).
//!
//! THE single rule deciding which upstream iteration a consumer NodeRun reads
//! its inputs from: **the latest iteration of the source node that
//! Completed**. Artifacts written by failed iterations stay on disk (forensic
//! value) but are never resolvable as inputs; a feeder that only ever ran at
//! iter 1 keeps serving its iter-1 artifact to consumers at any lap — it is
//! never dragged into a loop lap (#195/#199).
//!
//! [`resolve_consumer_inputs`] is the single edge-walk that turns a consumer's
//! incoming edges into concrete Blackboard paths, with that iteration decision
//! baked in. Every production resolution path projects over it — none walks the
//! edges itself (#370):
//! - `prompt_augmenter::resolve_input_paths` (spawn-time preamble + script env),
//! - `node_primitives::resolve_inputs` (manual `start_node` forensic payload),
//! - `node_io_resolver::resolve` (the inspector `/io` endpoint + `node_done`).
//!
//! This is a pure module: it takes the pre-resolved iteration maps (built by
//! [`resolved_source_iters`] / [`resolved_repeated_iters`] from a `RunState`)
//! rather than a `RunState` itself, so `prompt_augmenter` — which is pure and
//! cannot hold a `RunState` — shares the exact same walk. Callers holding a
//! `RunState` build the two maps first.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

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

/// One consumer input, fully resolved to concrete Blackboard path(s).
///
/// The neutral shape all three consumers project over (#370): the inspector
/// adds `FileInfo`/frontmatter + a disk stat, the preamble adds prose and reads
/// `from_start`, the forensic payload flattens `paths` to a `\n`-joined string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedInput {
    /// The consumer-side (target) port this input feeds.
    pub port: String,
    /// True when the source is the Start node — the input is the run's user
    /// prompt (`_input/output.md`), not a per-iteration artifact.
    pub from_start: bool,
    /// True when the edge accumulates across iterations (#149 / #353).
    pub repeated: bool,
    /// The concrete resolved path(s): exactly one for a single wire or a
    /// Start-sourced prompt; one per COMPLETED source iteration for a
    /// `repeated`/pooled input (empty when the source has completed none —
    /// never a raw `iter-*` glob).
    pub paths: Vec<PathBuf>,
}

/// THE single edge-walk turning a consumer's incoming edges into concrete input
/// paths, with the iteration decision delegated to [`source_iter`] /
/// completed-iters (#194 / #210 / #353). One [`ResolvedInput`] per incoming
/// edge, in edge-declaration order; pooling of same-named edges is a projection
/// concern left to each consumer.
///
/// Pure: it consumes the pre-resolved iteration maps — `source_iters` from
/// [`resolved_source_iters`] (covers every incoming edge) and `repeated_iters`
/// from [`resolved_repeated_iters`] (repeated edges only). A non-repeated source
/// absent from `source_iters` falls back to `consumer_iter` (positional), the
/// same fallback [`source_iter`] applies for override/injection flows; a
/// repeated source absent from `repeated_iters` pools nothing.
pub fn resolve_consumer_inputs(
    pipeline: &PipelineDef,
    artifacts_dir: &Path,
    node_id: &str,
    consumer_iter: i64,
    source_iters: &HashMap<String, i64>,
    repeated_iters: &HashMap<String, Vec<i64>>,
) -> Vec<ResolvedInput> {
    pipeline
        .edges
        .iter()
        .filter(|e| e.target.node == node_id)
        .map(|edge| {
            let source_node = &edge.source.node;
            // A Start-sourced edge reads the run's user prompt (`_input`), never
            // a per-iteration artifact — the canonical rule `prompt_augmenter`
            // has always applied; unified here so every consumer agrees (#370).
            let from_start = pipeline
                .nodes
                .iter()
                .any(|n| n.id == *source_node && n.node_type == crate::pipeline::NodeType::Start);

            let repeated = edge.repeated;
            let paths = if from_start {
                vec![crate::blackboard::input_path(artifacts_dir)]
            } else if repeated {
                // #353: one concrete path per COMPLETED source iteration.
                repeated_iters
                    .get(source_node.as_str())
                    .map(|iters| {
                        iters
                            .iter()
                            .map(|&n| {
                                crate::blackboard::artifact_path(
                                    artifacts_dir,
                                    source_node,
                                    n,
                                    &edge.source.port,
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                // #194 / #370: the source's latest COMPLETED iteration, never
                // the consumer's positional `iter` (which a cross-iteration or
                // external feeder never produced).
                let it = source_iters
                    .get(source_node.as_str())
                    .copied()
                    .unwrap_or(consumer_iter);
                vec![crate::blackboard::artifact_path(
                    artifacts_dir,
                    source_node,
                    it,
                    &edge.source.port,
                )]
            };

            ResolvedInput {
                port: edge.target.port.clone(),
                from_start,
                repeated,
                paths,
            }
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

    // -----------------------------------------------------------------------
    // resolve_consumer_inputs — THE single edge-walk all three consumers
    // (node_io_resolver, prompt_augmenter, node_primitives) project over (#370).
    // Asserting the iteration decision here asserts it for every consumer.
    // -----------------------------------------------------------------------

    use std::path::{Path, PathBuf};

    fn node_def(id: &str, node_type: crate::pipeline::NodeType) -> crate::pipeline::NodeDef {
        crate::pipeline::NodeDef {
            id: id.into(),
            name: id.into(),
            node_type,
            inputs: vec![],
            outputs: vec![],
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
            model: None,
        }
    }

    fn wire_pipeline(repeated: bool, source_type: crate::pipeline::NodeType) -> PipelineDef {
        use crate::pipeline::{EdgeDef, EdgeEndpoint};
        PipelineDef {
            name: "wire".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                node_def("planner", source_type),
                node_def("implementer", crate::pipeline::NodeType::CodeMutating),
            ],
            edges: vec![EdgeDef {
                source: EdgeEndpoint {
                    node: "planner".into(),
                    port: "plan".into(),
                },
                target: EdgeEndpoint {
                    node: "implementer".into(),
                    port: "plan".into(),
                },
                repeated,
                ..Default::default()
            }],
            loops: vec![],
            notes: Vec::new(),
            prompt_required: true,
        }
    }

    /// Build the two iteration maps the way every RunState-holding consumer does,
    /// then run the shared seam — exercising the exact production path.
    fn resolve_over_seam(
        pipeline: &PipelineDef,
        state: &RunState,
        artifacts: &Path,
        node_id: &str,
        consumer_iter: i64,
    ) -> Vec<ResolvedInput> {
        let source_iters = resolved_source_iters(pipeline, state, node_id, consumer_iter);
        let repeated_iters = resolved_repeated_iters(pipeline, state, node_id);
        resolve_consumer_inputs(
            pipeline,
            artifacts,
            node_id,
            consumer_iter,
            &source_iters,
            &repeated_iters,
        )
    }

    #[test]
    fn resolve_consumer_inputs_single_wire_reads_latest_completed_source() {
        // #370 core guard (RED→GREEN): a non-repeated edge whose source FAILED at
        // iter-1 then COMPLETED at iter-2 resolves to iter-2 — the source's
        // latest-completed iter — no matter the consumer's own iter. This is the
        // one decision node_io_resolver used to get wrong (it read the consumer's
        // iter positionally); all three consumers now inherit this assertion.
        let pipeline = wire_pipeline(false, crate::pipeline::NodeType::DocOnly);
        let state = state_with(vec![node_with_iterations(
            "planner",
            &[(1, NodeStatus::Failed), (2, NodeStatus::Completed)],
        )]);
        let artifacts = Path::new("/artifacts");

        // The consumer reads at lap 1 AND at lap 3 — both resolve to iter-2.
        for consumer_iter in [1, 3] {
            let resolved =
                resolve_over_seam(&pipeline, &state, artifacts, "implementer", consumer_iter);
            assert_eq!(resolved.len(), 1);
            assert_eq!(resolved[0].port, "plan");
            assert!(!resolved[0].repeated);
            assert!(!resolved[0].from_start);
            assert_eq!(
                resolved[0].paths,
                vec![PathBuf::from("/artifacts/planner/iter-2/plan/output.md")],
                "resolves to the source's latest-completed iter, not consumer_iter={consumer_iter}"
            );
        }
    }

    #[test]
    fn resolve_consumer_inputs_repeated_pools_only_completed_iters() {
        // #353 within the unified seam: a repeated edge enumerates one concrete
        // path per COMPLETED source iter (iter-2 failed → quarantined), ascending.
        let pipeline = wire_pipeline(true, crate::pipeline::NodeType::DocOnly);
        let state = state_with(vec![node_with_iterations(
            "planner",
            &[
                (1, NodeStatus::Completed),
                (2, NodeStatus::Failed),
                (3, NodeStatus::Completed),
            ],
        )]);
        let resolved =
            resolve_over_seam(&pipeline, &state, Path::new("/artifacts"), "implementer", 4);
        assert_eq!(resolved.len(), 1);
        assert!(resolved[0].repeated);
        assert_eq!(
            resolved[0].paths,
            vec![
                PathBuf::from("/artifacts/planner/iter-1/plan/output.md"),
                PathBuf::from("/artifacts/planner/iter-3/plan/output.md"),
            ]
        );
    }

    #[test]
    fn resolve_consumer_inputs_start_source_reads_the_run_prompt() {
        // A Start-sourced edge resolves to the run's `_input/output.md`, never a
        // per-iteration artifact — unified from prompt_augmenter so node_io and
        // node_primitives agree (#370).
        let pipeline = wire_pipeline(false, crate::pipeline::NodeType::Start);
        let state = state_with(vec![node_with_iterations(
            "planner",
            &[(1, NodeStatus::Completed)],
        )]);
        let resolved =
            resolve_over_seam(&pipeline, &state, Path::new("/artifacts"), "implementer", 1);
        assert_eq!(resolved.len(), 1);
        assert!(resolved[0].from_start);
        assert_eq!(
            resolved[0].paths,
            vec![PathBuf::from("/artifacts/_input/output.md")]
        );
    }

    #[test]
    fn resolve_consumer_inputs_falls_back_to_consumer_iter_when_source_unstarted() {
        // Override/injection flows: nothing completed → the path points where the
        // artifact will appear (positional consumer_iter), preserving prior
        // behaviour for start_node-ahead-of-deps.
        let pipeline = wire_pipeline(false, crate::pipeline::NodeType::DocOnly);
        let state = state_with(vec![node_with_iterations(
            "planner",
            &[(1, NodeStatus::Running)],
        )]);
        let resolved =
            resolve_over_seam(&pipeline, &state, Path::new("/artifacts"), "implementer", 2);
        assert_eq!(
            resolved[0].paths,
            vec![PathBuf::from("/artifacts/planner/iter-2/plan/output.md")]
        );
    }
}
