//! The loop regions of a pipeline as a document (ADR-0011): the region a
//! cycle is born with, its id, its bound, and the regions an edge removal
//! destroys. How a Run laps through a region belongs to the Run.

use std::collections::HashMap;

use super::cycles::detect_cycles;
use super::pipeline::{LoopKind, LoopRegion, NodeDef, NodeType, PipelineDef};

/// The cap given to an auto-materialized bounded region, so a drawn cycle is
/// never accidentally unbounded. Matches the daemon's `max_iter` fallback.
pub const DEFAULT_MAX_ITER: i64 = 5;

/// FNV-1a over the sorted member ids. Deriving the id from the member set —
/// rather than minting a random one — is what makes auto-materialization
/// *idempotent*: re-reading the same graph yields the same id, so the run's
/// per-region lap counter (`loop_states`, keyed by id) survives every reparse
/// and every daemon restart.
///
/// The editor's `lib/loopRegions.ts::generatedRegionId` is the byte-for-byte
/// mirror of this function (pinned on both sides by
/// `generated_region_id_matches_the_editor_mirror` and its vitest twin).
pub fn generated_region_id(members: &[String]) -> String {
    let mut sorted: Vec<&str> = members.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    for m in sorted {
        for b in m.bytes() {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(PRIME);
        }
        // Separator only — XOR, no multiply. The editor mirror does the same;
        // adding a round here would silently fork the two id spaces.
        hash ^= 0x2f;
    }
    format!("loop-{hash:08x}")
}

/// The bounded regions that must be materialized for the cycles a pipeline's
/// graph closes but its `loops:` block does not cover (ADR-0011 clause (b):
/// a `bounded` loop *is born by auto-detection of a cycle* "pour qu'aucun cycle
/// ne soit accidentellement non-borné"). A cycle is "covered" when an existing
/// region's member set is identical to it.
///
/// Called from [`parse_pipeline`](super::pipeline::parse_pipeline) so **every** reader — editor,
/// scheduler at run launch, library twin diff — sees the same bounded region
/// without anyone redrawing an edge. Never touches the file: the region is
/// derived at parse and persists only if the user saves. The editor mirror
/// (`lib/loopRegions.ts::materializeMissingRegions`) covers the live gesture.
///
/// **Legacy carve-out.** A cycle that runs through a legacy `type: loop` node is
/// skipped. That node already carries its own `max_iter` and its own iteration
/// path in the scheduler; wrapping it in a region would give the same laps two
/// competing counters, and would advertise `DEFAULT_MAX_ITER` on the canvas while
/// the engine ran the node's real bound (#396: the dial would lie). Those
/// pipelines are the migrator's job — `parse_pipeline` emits the
/// `run pdo migrate` diagnostic for them instead.
pub fn materialize_missing_regions(pipeline: &PipelineDef) -> Vec<LoopRegion> {
    let legacy_loop_nodes: std::collections::HashSet<&str> = pipeline
        .nodes
        .iter()
        .filter(|n| n.node_type == NodeType::Loop)
        .map(|n| n.id.as_str())
        .collect();

    let covered: Vec<std::collections::HashSet<&str>> = pipeline
        .loops
        .iter()
        .map(|r| r.members.iter().map(String::as_str).collect())
        .collect();

    let mut out = Vec::new();
    for cycle in detect_cycles(pipeline) {
        if cycle.iter().any(|m| legacy_loop_nodes.contains(m.as_str())) {
            continue;
        }
        let same_set = |c: &std::collections::HashSet<&str>| {
            c.len() == cycle.len() && cycle.iter().all(|m| c.contains(m.as_str()))
        };
        if covered.iter().any(same_set) {
            continue;
        }
        out.push(LoopRegion {
            id: generated_region_id(&cycle),
            kind: LoopKind::Bounded,
            members: cycle,
            max_iter: Some(serde_yaml::Value::Number(serde_yaml::Number::from(
                DEFAULT_MAX_ITER,
            ))),
            over: None,
        });
    }
    out
}

/// True when a bounded region's members still close a cycle under the
/// pipeline's *current* edges (ADR-0011 / #150). A region "closes a cycle" when
/// `detect_cycles` finds a cycle whose members are all members of the region —
/// the topological signature the region was materialized from. Removing the
/// edge that takes this to `false` removes the region's **last** cycle, which is
/// what triggers the destroy-loop confirmation.
fn region_has_cycle(pipeline: &PipelineDef, region: &LoopRegion) -> bool {
    let member_set: std::collections::HashSet<&str> =
        region.members.iter().map(String::as_str).collect();
    detect_cycles(pipeline)
        .iter()
        .any(|cycle| cycle.iter().all(|m| member_set.contains(m.as_str())))
}

/// Returns the ids of the `bounded` regions that would be **destroyed** by
/// removing the edge at `edge_index` (ADR-0011 / #150). A region is destroyed
/// when it currently closes a cycle but, with that edge gone, its members no
/// longer close any cycle — i.e. the deleted edge was the region's **last**
/// cycle. Deleting an edge while another cycle still closes the region leaves
/// the loop intact (it is not returned). `collection` regions have no
/// topological cycle to lose and are never returned.
///
/// This is the destroy-vs-keep decision behind the confirmation popup: on
/// confirm, the caller removes each returned region's `loops:` entry (its bound
/// and iteration state go with it); deleting a non-last cycle edge returns an
/// empty list and pops nothing.
pub fn regions_destroyed_by_edge_removal(pipeline: &PipelineDef, edge_index: usize) -> Vec<String> {
    if edge_index >= pipeline.edges.len() {
        return Vec::new();
    }
    let mut without = pipeline.clone();
    without.edges.remove(edge_index);

    pipeline
        .loops
        .iter()
        .filter(|region| region.kind == LoopKind::Bounded)
        .filter(|region| region_has_cycle(pipeline, region) && !region_has_cycle(&without, region))
        .map(|region| region.id.clone())
        .collect()
}

/// Resolves a bounded region's `max_iter` (ADR-0011 / #148). A literal number is
/// the cap; a `$var` reference reads `vars`; anything else (or a missing cap)
/// falls back to [`DEFAULT_MAX_ITER`] so a region is never accidentally
/// unbounded. Mirrors [`resolve_node_max_iter`] for the legacy Loop node so
/// region and node iteration agree on the same bound semantics.
pub fn resolve_region_max_iter(
    region: &LoopRegion,
    vars: &HashMap<String, serde_yaml::Value>,
) -> i64 {
    match &region.max_iter {
        Some(serde_yaml::Value::Number(n)) => n.as_i64().unwrap_or(DEFAULT_MAX_ITER),
        Some(serde_yaml::Value::String(s)) => {
            if let Some(var_name) = s.strip_prefix('$') {
                vars.get(var_name)
                    .and_then(|v| v.as_i64())
                    .unwrap_or(DEFAULT_MAX_ITER)
            } else {
                s.parse::<i64>().unwrap_or(DEFAULT_MAX_ITER)
            }
        }
        _ => DEFAULT_MAX_ITER,
    }
}

/// Resolves a legacy `type: loop` node's `max_iter`, with the same semantics
/// as a region's: a number, a `$var` read from `vars`, else [`DEFAULT_MAX_ITER`].
pub fn resolve_node_max_iter(
    loop_node: &NodeDef,
    vars: &HashMap<String, serde_yaml::Value>,
) -> i64 {
    match &loop_node.max_iter {
        Some(serde_yaml::Value::Number(n)) => n.as_i64().unwrap_or(DEFAULT_MAX_ITER),
        Some(serde_yaml::Value::String(s)) => {
            if let Some(var_name) = s.strip_prefix('$') {
                vars.get(var_name)
                    .and_then(|v| v.as_i64())
                    .unwrap_or(DEFAULT_MAX_ITER)
            } else {
                s.parse::<i64>().unwrap_or(DEFAULT_MAX_ITER)
            }
        }
        _ => DEFAULT_MAX_ITER,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::pipeline::{LoopKind, NodeDef, NodeType, Port, PortType};
    use pretty_assertions::assert_eq;

    fn node(id: &str, inputs: &[&str], outputs: &[&str]) -> NodeDef {
        NodeDef {
            free_live_artifacts: false,
            live_artifacts: Vec::new(),
            skills: Vec::new(),
            isolated_worktree: None,
            id: id.into(),
            name: id.into(),
            node_type: NodeType::Agent,
            inputs: inputs
                .iter()
                .map(|n| Port {
                    name: (*n).into(),
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                })
                .collect(),
            outputs: outputs
                .iter()
                .map(|n| Port {
                    name: (*n).into(),
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                })
                .collect(),
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
            pin_harness: None,
            harnesses: Default::default(),
            agent_choice: None,
            auto_fail: None,
            orchestrator: false,
            on_demand: false,
        }
    }

    fn edge(sn: &str, sp: &str, tn: &str, tp: &str) -> crate::domain::pipeline::EdgeDef {
        crate::domain::pipeline::EdgeDef {
            source: crate::domain::pipeline::EdgeSource::single(sn, sp),
            target: crate::domain::pipeline::EdgeEndpoint {
                node: tn.into(),
                port: tp.into(),
            },
            ..Default::default()
        }
    }

    /// start -> impl -> rev -> impl (back-edge). One region: [impl, rev].
    fn review_loop() -> (PipelineDef, LoopRegion) {
        let pipeline = PipelineDef {
            name: "rl".into(),
            version: None,
            variables: Default::default(),
            nodes: vec![
                node("start", &[], &["user_prompt"]),
                node("impl", &["task", "review"], &["code"]),
                node("rev", &["code"], &["review"]),
            ],
            edges: vec![
                edge("start", "user_prompt", "impl", "task"),
                edge("impl", "code", "rev", "code"),
                edge("rev", "review", "impl", "review"),
            ],
            loops: vec![],
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
        };
        let region = LoopRegion {
            id: "review_loop".into(),
            kind: LoopKind::Bounded,
            members: vec!["impl".into(), "rev".into()],
            max_iter: Some(serde_yaml::Value::Number(3.into())),
            over: None,
        };
        (pipeline, region)
    }

    // ── Destroy-vs-keep on edge removal (ADR-0011 / #150) ─────────────────────

    #[test]
    fn deleting_the_only_back_edge_destroys_the_region() {
        // The bare review loop has a single cycle (rev -> impl). Deleting that
        // back-edge removes the region's last cycle, so the region is destroyed:
        // its `loops:` entry (and its bound + iteration state) go with it.
        let (mut pipeline, region) = review_loop();
        pipeline.loops = vec![region];
        // Edge order: 0 start->impl, 1 impl->rev, 2 rev->impl (the back-edge).
        let destroyed = regions_destroyed_by_edge_removal(&pipeline, 2);
        assert_eq!(destroyed, vec!["review_loop".to_string()]);
    }

    #[test]
    fn deleting_a_non_last_cycle_edge_keeps_the_region() {
        // A region with TWO cycles closing it (rev -> impl AND rev -> mid -> impl
        // via a second member). Deleting one back-edge leaves the other cycle, so
        // the region survives: no destroy, no popup.
        let pipeline = PipelineDef {
            name: "rl2".into(),
            version: None,
            variables: Default::default(),
            nodes: vec![
                node("start", &[], &["user_prompt"]),
                node("impl", &["task", "review", "more"], &["code"]),
                node("rev", &["code"], &["review", "extra"]),
                node("mid", &["extra"], &["more"]),
            ],
            edges: vec![
                edge("start", "user_prompt", "impl", "task"), // 0
                edge("impl", "code", "rev", "code"),          // 1
                edge("rev", "review", "impl", "review"),      // 2 back-edge A
                edge("rev", "extra", "mid", "extra"),         // 3
                edge("mid", "more", "impl", "more"),          // 4 back-edge B
            ],
            loops: vec![LoopRegion {
                id: "review_loop".into(),
                kind: LoopKind::Bounded,
                members: vec!["impl".into(), "rev".into(), "mid".into()],
                max_iter: Some(serde_yaml::Value::Number(3.into())),
                over: None,
            }],
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
        };
        // Deleting back-edge A (index 2) still leaves the impl->rev->mid->impl
        // cycle, so the region is kept.
        assert!(regions_destroyed_by_edge_removal(&pipeline, 2).is_empty());
    }

    #[test]
    fn deleting_an_edge_outside_any_cycle_destroys_nothing() {
        // Deleting a purely-forward edge (start -> impl, index 0) does not touch
        // the region's cycle, so no region is destroyed (no popup).
        let (mut pipeline, region) = review_loop();
        pipeline.loops = vec![region];
        assert!(regions_destroyed_by_edge_removal(&pipeline, 0).is_empty());
    }

    #[test]
    fn a_collection_region_is_never_destroyed_by_edge_removal() {
        // A `collection` region has no topological cycle to lose; removing any
        // edge never pops the bounded-region destroy confirmation.
        let (mut pipeline, _region) = collection_fanout_convergence();
        pipeline.loops = vec![LoopRegion {
            id: "per-issue".into(),
            kind: LoopKind::Collection,
            members: vec!["fixer".into()],
            max_iter: None,
            over: Some("issues".into()),
        }];
        for i in 0..pipeline.edges.len() {
            assert!(
                regions_destroyed_by_edge_removal(&pipeline, i).is_empty(),
                "collection region must never be destroyed (edge {i})"
            );
        }
    }

    // ── Collection region engine (ADR-0011 / #151) ──────────────────────────

    /// triage -> fixer (single-member collection over `issues`) -> gather -> end.
    /// The barrier edge leaves the region (fixer:fix -> gather:branches); `gather`
    /// is an ordinary agent converging the laps (ADR-0082).
    fn collection_fanout_convergence() -> (PipelineDef, LoopRegion) {
        let pipeline = PipelineDef {
            name: "cfm".into(),
            version: None,
            variables: Default::default(),
            nodes: vec![
                node("start", &[], &["user_prompt"]),
                node("triage", &["task"], &["plan"]),
                node("fixer", &["in"], &["fix"]),
                node("gather", &["branches"], &["merged"]),
                node("end", &["result"], &[]),
            ],
            edges: vec![
                edge("start", "user_prompt", "triage", "task"),
                edge("triage", "plan", "fixer", "in"),
                edge("fixer", "fix", "gather", "branches"),
                edge("gather", "merged", "end", "result"),
            ],
            loops: vec![],
            notes: Vec::new(),
            prompt_required: true,
            grid_size: None,
            wait_for_started_nodes: false,
        };
        let region = LoopRegion {
            id: "per-issue".into(),
            kind: LoopKind::Collection,
            members: vec!["fixer".into()],
            max_iter: None,
            over: Some("issues".into()),
        };
        (pipeline, region)
    }

    // --- Auto-materialization at the model boundary (ADR-0011 (b) / #396) ---

    #[test]
    fn generated_region_id_matches_the_editor_mirror() {
        // Pinned against the value `lib/loopRegions.ts::generatedRegionId` produces
        // for the same members (its vitest twin asserts the same literal). The two
        // implementations must not drift: a region auto-materialized on the canvas
        // and the same region materialized here have to be ONE region, or a save
        // would duplicate it and the run's lap counter would key off the wrong id.
        assert_eq!(
            generated_region_id(&["impl".to_string(), "rev".to_string()]),
            "loop-2e1c629fbe6d531a"
        );
        // Member order is not identity: the id is derived from the sorted set.
        assert_eq!(
            generated_region_id(&["rev".to_string(), "impl".to_string()]),
            generated_region_id(&["impl".to_string(), "rev".to_string()])
        );
        assert_eq!(
            generated_region_id(&["aaaa1111".to_string()]),
            "loop-4f5ab6d49af12362"
        );
    }

    #[test]
    fn materializes_a_bounded_region_for_an_undeclared_cycle() {
        // The #396 core: a graph closing a cycle with an empty `loops:` block gets
        // its bounded region derived, at DEFAULT_MAX_ITER, so the cycle is never
        // accidentally unbounded and the canvas has a region to draw.
        let (pipeline, _) = review_loop();
        let regions = materialize_missing_regions(&pipeline);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].kind, LoopKind::Bounded);
        assert_eq!(
            regions[0].members,
            vec!["impl".to_string(), "rev".to_string()]
        );
        assert_eq!(
            resolve_region_max_iter(&regions[0], &Default::default()),
            DEFAULT_MAX_ITER
        );
        assert_eq!(regions[0].id, generated_region_id(&regions[0].members));
    }

    #[test]
    fn materialization_is_idempotent() {
        // Feeding the derived region back in must add nothing — the id is a pure
        // function of the member set, so re-parsing the same file (every run
        // advance re-reads the snapshot) cannot grow a second region nor rename
        // the one whose lap counter is live.
        let (mut pipeline, _) = review_loop();
        pipeline.loops = materialize_missing_regions(&pipeline);
        assert!(materialize_missing_regions(&pipeline).is_empty());
    }

    #[test]
    fn a_declared_region_is_left_alone() {
        // The persisted `loops:` entry keeps its own id AND its own bound: covering
        // is by member set, so no duplicate region is derived next to it.
        let (mut pipeline, region) = review_loop();
        pipeline.loops = vec![region];
        assert!(materialize_missing_regions(&pipeline).is_empty());
    }

    #[test]
    fn a_cycle_through_a_legacy_loop_node_is_left_to_the_migrator() {
        // A `type: loop` node carries its own `max_iter` and its own iteration path
        // in the scheduler. Wrapping its cycle in a region would give one loop two
        // counters, and would advertise DEFAULT_MAX_ITER on a canvas whose engine
        // runs the node's real bound — a dial that lies (#396). Those files are
        // `pdo migrate`'s job; `parse_pipeline` diagnoses them instead.
        let (mut pipeline, _) = review_loop();
        pipeline.nodes.push({
            let mut n = node("lp", &["in", "break"], &["body", "done"]);
            n.node_type = NodeType::Loop;
            n.max_iter = Some(serde_yaml::Value::Number(3.into()));
            n
        });
        // rev -> lp -> impl closes a second, larger cycle through the loop node.
        pipeline.edges.push(edge("rev", "review", "lp", "break"));
        pipeline.edges.push(edge("lp", "body", "impl", "task"));

        let cycles = crate::domain::cycles::detect_cycles(&pipeline);
        assert!(
            cycles.iter().any(|c| c.iter().any(|m| m == "lp")),
            "fixture must actually close a cycle through the loop node: {cycles:?}"
        );
        for region in materialize_missing_regions(&pipeline) {
            assert!(
                !region.members.iter().any(|m| m == "lp"),
                "no region may cover a legacy loop node: {region:?}"
            );
        }
    }
}
