//! Bounded loop region engine (ADR-0011 / #148).
//!
//! A bounded loop region replaces the old `Loop` node. The region is a *named*
//! entry of the pipeline `loops:` block (`id` + `kind: bounded` + `members` +
//! `max_iter`); its iteration counter is **region-wide, keyed by `id`**.
//!
//! This module is the pure decision core: given a region, its current runtime
//! counter, and which re-entry (back) edges fired in the just-completed lap, it
//! decides whether to start the next lap — spawning the region entry **once**,
//! even when several back-edges fire (coalesced; fixes the iter+1 double-spawn,
//! #108) — or to enter the explicit `Exhausted` state at `max_iter`. It never
//! produces a silent stall.

use crate::graph_resolver;
use crate::pipeline::{LoopKind, LoopRegion, PipelineDef};

/// The default iteration cap given to an auto-materialized bounded region, so a
/// drawn cycle is never accidentally unbounded (ADR-0011 / #148). Matches the
/// daemon's existing `max_iter` fallback.
pub const DEFAULT_MAX_ITER: i64 = 5;

/// The live per-region iteration counter (keyed by the region `id` elsewhere).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionRuntime {
    pub current_iter: i64,
    pub max_iter: i64,
    pub exhausted: bool,
}

impl RegionRuntime {
    /// A region begins at lap 1.
    pub fn new(max_iter: i64) -> Self {
        Self {
            current_iter: 1,
            max_iter,
            exhausted: false,
        }
    }
}

/// The outcome of resolving a completed lap's re-entry signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LapDecision {
    /// Start the next lap: bump the counter to `iter` and re-spawn `entry` once.
    NextLap { iter: i64, entry: String },
    /// `max_iter` reached with re-entry still requested: the region is exhausted.
    /// The caller routes an `iter >= max` exit edge if one matches, else blocks
    /// the region "exhausted — unrouted" (never a silent stall).
    Exhausted,
    /// No re-entry edge fired: the region does not loop again this turn.
    NoReentry,
}

/// Resolves one lap of a bounded region given how many re-entry edges fired.
///
/// `reentry_fired` is the number of back-edges (edges from a member back into the
/// region) that fired in the completed lap. Any positive count is **coalesced**
/// into a single next-lap entry-spawn: firing two back-edges in one lap must not
/// advance the counter twice nor spawn the entry twice (#108 regression).
pub fn resolve_lap(
    pipeline: &PipelineDef,
    region: &LoopRegion,
    runtime: &RegionRuntime,
    reentry_fired: usize,
) -> LapDecision {
    if reentry_fired == 0 {
        return LapDecision::NoReentry;
    }
    if runtime.current_iter >= runtime.max_iter {
        return LapDecision::Exhausted;
    }
    let entry = match graph_resolver::region_entry(pipeline, &region.members) {
        Some(e) => e,
        // A closed island with no external entry: fall back to the first member
        // so the lap still advances deterministically rather than stalling.
        None => region.members.first().cloned().unwrap_or_default(),
    };
    LapDecision::NextLap {
        iter: runtime.current_iter + 1,
        entry,
    }
}

/// Returns the global indices (into `pipeline.edges`) of a region's *re-entry*
/// edges: edges whose source is a member and whose target is the region entry.
/// These are the back-edges whose firing requests another lap. No edge is
/// flagged a "back-edge" in the YAML — the role is derived from the region
/// topology (ADR-0011).
pub fn reentry_edge_indices(pipeline: &PipelineDef, region: &LoopRegion) -> Vec<usize> {
    let entry = match graph_resolver::region_entry(pipeline, &region.members) {
        Some(e) => e,
        None => match region.members.first() {
            Some(m) => m.clone(),
            None => return Vec::new(),
        },
    };
    let member_set: std::collections::HashSet<&str> =
        region.members.iter().map(String::as_str).collect();
    pipeline
        .edges
        .iter()
        .enumerate()
        .filter(|(_, e)| member_set.contains(e.source.node.as_str()) && e.target.node == entry)
        .map(|(i, _)| i)
        .collect()
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
    graph_resolver::detect_cycles(pipeline)
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

/// Builds the per-iteration edge-resolution key (ADR-0011 / #148 scheduler
/// concern). The model from commit da0d72e resolves convergence edges (fired /
/// dead) for the out-of-loop case; inside a region the resolution state must be
/// keyed by `(loop id, iter, edge)` so an edge that fired at lap 1 is not counted
/// resolved at lap 2.
pub fn resolution_key(loop_id: &str, iter: i64, edge_index: usize) -> String {
    format!("{loop_id}#{iter}#{edge_index}")
}

/// Resolves a bounded region's `max_iter` (ADR-0011 / #148). A literal number is
/// the cap; a `$var` reference reads `vars`; anything else (or a missing cap)
/// falls back to [`DEFAULT_MAX_ITER`] so a region is never accidentally
/// unbounded. Mirrors `scheduler::resolve_max_iter` for the legacy Loop node so
/// region and node iteration agree on the same bound semantics.
pub fn resolve_region_max_iter(
    region: &LoopRegion,
    vars: &std::collections::HashMap<String, serde_yaml::Value>,
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

/// The bounded region a `node_id` is an entry of, *and* the given edge re-enters
/// (ADR-0011 / #148). Used by the scheduler to recognise that a fired edge into
/// `target` is a region re-entry (back-edge) rather than a plain forward edge, so
/// region iteration / exhaustion governs the spawn instead of the generic path.
/// Returns the first matching `bounded` region whose entry is `target` and whose
/// members include `source` (the back-edge `source -> target` closes the region).
pub fn bounded_region_reentered_by_edge<'a>(
    pipeline: &'a PipelineDef,
    source: &str,
    target: &str,
) -> Option<&'a LoopRegion> {
    pipeline.loops.iter().find(|region| {
        region.kind == LoopKind::Bounded
            && region.members.iter().any(|m| m == source)
            && reentry_edge_indices(pipeline, region).iter().any(|&i| {
                let e = &pipeline.edges[i];
                e.source.node == source && e.target.node == target
            })
    })
}

/// The outcome of an exhausted bounded region (ADR-0011 / #148).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExhaustionOutcome {
    /// An `iter >= max` (or otherwise matching) exit edge routes the exhaustion
    /// to one or more external targets. Targets are de-duplicated, in edge order.
    Routed(Vec<String>),
    /// No exit edge matched: the region is blocked "exhausted — unrouted"
    /// (routable by the Pipeline Manager), never a silent stall.
    Unrouted,
}

/// Decides what happens when a bounded region reaches `max_iter` with re-entry
/// still requested. Evaluates each member's outgoing edges at `iter = max_iter`
/// (reusing the conditional-edge router, so an `iter >= max` guard fires) and
/// collects the edges leaving the region (member → non-member). If any fire, the
/// exhaustion is `Routed` to their external targets; otherwise it is `Unrouted`.
pub fn exhaustion_outcome(
    pipeline: &PipelineDef,
    region: &LoopRegion,
    runtime: &RegionRuntime,
    frontmatter: &std::collections::HashMap<String, serde_yaml::Value>,
    vars: &std::collections::HashMap<String, serde_yaml::Value>,
) -> ExhaustionOutcome {
    let member_set: std::collections::HashSet<&str> =
        region.members.iter().map(String::as_str).collect();

    let mut targets: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for member in &region.members {
        let outgoing: Vec<&crate::pipeline::EdgeDef> = pipeline
            .edges
            .iter()
            .filter(|e| e.source.node == *member)
            .collect();
        let fired =
            crate::edge_router::fired_edges(&outgoing, frontmatter, vars, runtime.current_iter);
        for e in fired {
            if !member_set.contains(e.target.node.as_str()) && seen.insert(e.target.node.clone()) {
                targets.push(e.target.node.clone());
            }
        }
    }

    if targets.is_empty() {
        ExhaustionOutcome::Unrouted
    } else {
        ExhaustionOutcome::Routed(targets)
    }
}

// ── Collection region engine (ADR-0011 / #151) ───────────────────────────────
//
// A `collection` region (ex-ForEach) carries an `over: <field>` driver naming a
// list in the entering artifact's frontmatter. It fans the region entry out **in
// parallel**, one lap per item; the region's outgoing edges fire **once, on the
// barrier** — when every item finishes — preserving `done → Merge` convergence
// (ADR-0006). An empty collection fires the barrier immediately with zero
// item-artifacts. The model concept is the named loop; "region" is the canvas
// rendering.

/// Resolves a collection region's driver list from the entering artifact's
/// frontmatter. The list is the value of the region's `over` field; a missing or
/// non-list field resolves to the empty collection (sharp tool, ADR-0001 — no
/// error; an empty collection simply fires the barrier immediately). Mirrors the
/// legacy `scheduler::foreach_resolve_collection` so the collection region and
/// the retired ForEach node agree on resolution.
pub fn resolve_collection(
    region: &LoopRegion,
    frontmatter: &std::collections::HashMap<String, serde_yaml::Value>,
) -> Vec<serde_yaml::Value> {
    let over = match region.over.as_deref() {
        Some(o) => o,
        None => return Vec::new(),
    };
    frontmatter
        .get(over)
        .and_then(|v| v.as_sequence())
        .cloned()
        .unwrap_or_default()
}

/// The fan-out plan for a collection region (ADR-0011 / #151). `total` is the
/// collection size (number of laps); `entry` is the region member spawned once
/// per item; `items` is the resolved driver list (deposited so each lap reads its
/// own item). An empty collection has `total == 0` and no entry spawns — the
/// caller fires the barrier immediately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionFanout {
    pub total: i64,
    pub entry: String,
    pub items: Vec<serde_yaml::Value>,
}

/// Plans the parallel fan-out of a collection region from the entering artifact's
/// frontmatter (ADR-0011 / #151). Resolves the `over` list, then designates the
/// region entry (the member fed from outside the region; for the common
/// single-member collection that is the lone member) as the node spawned **once
/// per item**, at laps `1..=total`. An empty collection yields `total == 0` and
/// no spawns; the caller fires the barrier immediately.
pub fn collection_fanout(
    pipeline: &PipelineDef,
    region: &LoopRegion,
    frontmatter: &std::collections::HashMap<String, serde_yaml::Value>,
) -> CollectionFanout {
    let items = resolve_collection(region, frontmatter);
    let total = items.len() as i64;
    let entry = match graph_resolver::region_entry(pipeline, &region.members) {
        Some(e) => e,
        None => region.members.first().cloned().unwrap_or_default(),
    };
    CollectionFanout {
        total,
        entry,
        items,
    }
}

/// True once every item-lap of a collection region has completed (ADR-0011 /
/// #151) — the **barrier**. `total` is the collection size; `completed_iters` is
/// the set of laps whose every member has finished. The barrier is reached when
/// laps `1..=total` are all complete. An empty collection (`total == 0`) is
/// barriered by definition (vacuously), so the caller fires immediately.
pub fn collection_barrier_reached(
    total: i64,
    completed_iters: &std::collections::HashSet<i64>,
) -> bool {
    if total == 0 {
        return true;
    }
    (1..=total).all(|i| completed_iters.contains(&i))
}

/// The external targets a collection region's barrier fires into (ADR-0011 /
/// #151): the edges leaving the region (member → non-member). Fired **once**, in
/// edge order, de-duplicated — preserving `done → Merge` convergence (ADR-0006).
/// Collection-region outgoing edges are unconditional barriers (the lap count is
/// the collection, not a guard), so every member→non-member edge fires.
pub fn collection_barrier_targets(pipeline: &PipelineDef, region: &LoopRegion) -> Vec<String> {
    let member_set: std::collections::HashSet<&str> =
        region.members.iter().map(String::as_str).collect();
    let mut targets: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for edge in &pipeline.edges {
        if member_set.contains(edge.source.node.as_str())
            && !member_set.contains(edge.target.node.as_str())
            && seen.insert(edge.target.node.clone())
        {
            targets.push(edge.target.node.clone());
        }
    }
    targets
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{LoopKind, NodeDef, NodeType, Port, PortType};
    use pretty_assertions::assert_eq;

    fn node(id: &str, inputs: &[&str], outputs: &[&str]) -> NodeDef {
        NodeDef {
            id: id.into(),
            name: id.into(),
            node_type: NodeType::DocOnly,
            inputs: inputs
                .iter()
                .map(|n| Port {
                    name: (*n).into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                })
                .collect(),
            outputs: outputs
                .iter()
                .map(|n| Port {
                    name: (*n).into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                })
                .collect(),
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
            model: None,
        }
    }

    fn edge(sn: &str, sp: &str, tn: &str, tp: &str) -> crate::pipeline::EdgeDef {
        crate::pipeline::EdgeDef {
            source: crate::pipeline::EdgeEndpoint {
                node: sn.into(),
                port: sp.into(),
            },
            target: crate::pipeline::EdgeEndpoint {
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

    #[test]
    fn one_back_edge_starts_the_next_lap_at_the_entry() {
        let (pipeline, region) = review_loop();
        let runtime = RegionRuntime::new(3);
        let decision = resolve_lap(&pipeline, &region, &runtime, 1);
        assert_eq!(
            decision,
            LapDecision::NextLap {
                iter: 2,
                entry: "impl".into()
            }
        );
    }

    #[test]
    fn multiple_back_edges_coalesce_into_one_next_lap() {
        // #108 regression: two back-edges firing in the SAME lap must advance the
        // counter exactly once and re-spawn the entry exactly once — not iter+2,
        // not two spawns.
        let (pipeline, region) = review_loop();
        let runtime = RegionRuntime::new(5);
        let decision = resolve_lap(&pipeline, &region, &runtime, 2);
        assert_eq!(
            decision,
            LapDecision::NextLap {
                iter: 2,
                entry: "impl".into()
            }
        );
    }

    #[test]
    fn no_back_edge_means_no_reentry() {
        let (pipeline, region) = review_loop();
        let runtime = RegionRuntime::new(3);
        assert_eq!(
            resolve_lap(&pipeline, &region, &runtime, 0),
            LapDecision::NoReentry
        );
    }

    #[test]
    fn reentry_at_max_iter_is_exhausted() {
        // At iter == max_iter with re-entry still requested, the region exhausts
        // (never silently advances past the bound).
        let (pipeline, region) = review_loop();
        let runtime = RegionRuntime {
            current_iter: 3,
            max_iter: 3,
            exhausted: false,
        };
        assert_eq!(
            resolve_lap(&pipeline, &region, &runtime, 1),
            LapDecision::Exhausted
        );
    }

    #[test]
    fn reentry_edges_are_member_to_member_into_the_entry() {
        // A re-entry (back) edge of a region is one whose source is a member and
        // whose target is the region entry. start->impl (external) is NOT one;
        // impl->rev (intra-body, not into entry) is NOT one; rev->impl IS one.
        let (pipeline, region) = review_loop();
        let backs = reentry_edge_indices(&pipeline, &region);
        // Edge order: 0 start->impl, 1 impl->rev, 2 rev->impl.
        assert_eq!(backs, vec![2]);
    }

    #[test]
    fn edge_resolution_key_is_per_loop_and_iter() {
        // The same back-edge resolves independently each lap: a key for (loop,
        // iter=1, edge) differs from (loop, iter=2, edge), so a lap-1 firing is
        // not counted resolved at lap 2.
        let k1 = resolution_key("review_loop", 1, 2);
        let k2 = resolution_key("review_loop", 2, 2);
        assert_ne!(k1, k2);
    }

    /// review loop with a designer-wired exhaustion exit: rev -> end when
    /// iter >= 3, plus the rev -> impl back-edge (unconditional here).
    fn review_loop_with_exit() -> (PipelineDef, LoopRegion) {
        let pipeline = PipelineDef {
            name: "rle".into(),
            version: None,
            variables: Default::default(),
            nodes: vec![
                node("start", &[], &["user_prompt"]),
                node("impl", &["task", "review"], &["code"]),
                node("rev", &["code"], &["review"]),
                node("end", &["result"], &[]),
            ],
            edges: vec![
                edge("start", "user_prompt", "impl", "task"),
                edge("impl", "code", "rev", "code"),
                edge("rev", "review", "impl", "review"),
                {
                    let mut e = edge("rev", "review", "end", "result");
                    e.when = Some(serde_yaml::from_str("iter: { gte: 3 }").unwrap());
                    e
                },
            ],
            loops: vec![],
            notes: Vec::new(),
            prompt_required: true,
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

    #[test]
    fn exhaustion_routes_through_a_matching_exit_edge() {
        // At iter == max with an `iter >= max` exit edge wired, exhaustion routes
        // to that edge's external target instead of blocking.
        let (pipeline, region) = review_loop_with_exit();
        let runtime = RegionRuntime {
            current_iter: 3,
            max_iter: 3,
            exhausted: false,
        };
        let outcome = exhaustion_outcome(
            &pipeline,
            &region,
            &runtime,
            &Default::default(),
            &Default::default(),
        );
        assert_eq!(outcome, ExhaustionOutcome::Routed(vec!["end".into()]));
    }

    #[test]
    fn exhaustion_with_no_matching_exit_is_unrouted() {
        // No exit edge wired (the bare review loop) → exhausted-unrouted, never a
        // silent stall.
        let (pipeline, region) = review_loop();
        let runtime = RegionRuntime {
            current_iter: 3,
            max_iter: 3,
            exhausted: false,
        };
        let outcome = exhaustion_outcome(
            &pipeline,
            &region,
            &runtime,
            &Default::default(),
            &Default::default(),
        );
        assert_eq!(outcome, ExhaustionOutcome::Unrouted);
    }

    #[test]
    fn single_member_self_loop_reenters_on_itself() {
        // A one-member self-looping region re-spawns that member as the entry.
        let pipeline = PipelineDef {
            name: "sl".into(),
            version: None,
            variables: Default::default(),
            nodes: vec![
                node("start", &[], &["user_prompt"]),
                node("worker", &["seed", "again"], &["out"]),
            ],
            edges: vec![
                edge("start", "user_prompt", "worker", "seed"),
                edge("worker", "out", "worker", "again"),
            ],
            loops: vec![],
            notes: Vec::new(),
            prompt_required: true,
        };
        let region = LoopRegion {
            id: "spin".into(),
            kind: LoopKind::Bounded,
            members: vec!["worker".into()],
            max_iter: Some(serde_yaml::Value::Number(4.into())),
            over: None,
        };
        let runtime = RegionRuntime::new(4);
        assert_eq!(
            resolve_lap(&pipeline, &region, &runtime, 1),
            LapDecision::NextLap {
                iter: 2,
                entry: "worker".into()
            }
        );
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
        let (mut pipeline, _region) = collection_fanout_merge();
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

    // ── Integration-style: a full bounded-region run via the engine ──────────
    //
    // Mirrors the migrated review-loop: rev -> end WHEN verdict in [PASS], and
    // rev -> impl ELSE (the continuation back-edge). The "scheduler" here is the
    // loop below: each lap evaluates rev's outgoing edges, then either re-enters
    // (back-edge fired) or exits / exhausts.

    fn migrated_review_loop(max_iter: i64) -> (PipelineDef, LoopRegion) {
        let pipeline = PipelineDef {
            name: "mrl".into(),
            version: None,
            variables: Default::default(),
            nodes: vec![
                node("start", &[], &["user_prompt"]),
                node("impl", &["task", "review"], &["code"]),
                node("rev", &["code"], &["review"]),
                node("end", &["result"], &[]),
            ],
            edges: vec![
                edge("start", "user_prompt", "impl", "task"),
                edge("impl", "code", "rev", "code"),
                {
                    let mut e = edge("rev", "review", "end", "result");
                    e.when =
                        Some(serde_yaml::from_str("verdict: { in: [PASS, APPROVED] }").unwrap());
                    e
                },
                {
                    let mut e = edge("rev", "review", "impl", "task");
                    e.is_else = true;
                    e
                },
            ],
            loops: vec![],
            notes: Vec::new(),
            prompt_required: true,
        };
        let region = LoopRegion {
            id: "review_loop".into(),
            kind: LoopKind::Bounded,
            members: vec!["impl".into(), "rev".into()],
            max_iter: Some(serde_yaml::Value::Number(max_iter.into())),
            over: None,
        };
        (pipeline, region)
    }

    /// Simulates running the region: `verdicts[i]` is the reviewer verdict at lap
    /// i+1. Returns `(laps_run, final_outcome)` where outcome is one of
    /// "exit:<target>", "exhausted-unrouted", "exhausted-routed:<target>".
    fn run_region(
        pipeline: &PipelineDef,
        region: &LoopRegion,
        max_iter: i64,
        verdicts: &[&str],
    ) -> (i64, String) {
        let mut runtime = RegionRuntime::new(max_iter);
        let reentry = reentry_edge_indices(pipeline, region);
        loop {
            let lap = runtime.current_iter as usize;
            let verdict = verdicts.get(lap - 1).copied().unwrap_or("FAIL");
            let mut fields = std::collections::HashMap::new();
            fields.insert(
                "verdict".to_string(),
                serde_yaml::Value::String(verdict.to_string()),
            );

            // Evaluate rev's outgoing edges at this iter.
            let rev_out: Vec<&crate::pipeline::EdgeDef> = pipeline
                .edges
                .iter()
                .filter(|e| e.source.node == "rev")
                .collect();
            let fired = crate::edge_router::fired_edges(
                &rev_out,
                &fields,
                &Default::default(),
                runtime.current_iter,
            );

            // Count fired re-entry edges (by identity against the reentry set).
            let reentry_fired = pipeline
                .edges
                .iter()
                .enumerate()
                .filter(|(i, e)| reentry.contains(i) && fired.iter().any(|f| std::ptr::eq(*f, *e)))
                .count();

            // An exit fires if any fired edge leaves the region.
            let member_set: std::collections::HashSet<&str> =
                region.members.iter().map(String::as_str).collect();
            let exit_target = fired
                .iter()
                .find(|e| !member_set.contains(e.target.node.as_str()))
                .map(|e| e.target.node.clone());

            if let Some(t) = exit_target {
                return (runtime.current_iter, format!("exit:{t}"));
            }

            match resolve_lap(pipeline, region, &runtime, reentry_fired) {
                LapDecision::NextLap { iter, .. } => {
                    runtime.current_iter = iter;
                }
                LapDecision::Exhausted => {
                    return match exhaustion_outcome(
                        pipeline,
                        region,
                        &runtime,
                        &fields,
                        &Default::default(),
                    ) {
                        ExhaustionOutcome::Routed(targets) => (
                            runtime.current_iter,
                            format!("exhausted-routed:{}", targets.join(",")),
                        ),
                        ExhaustionOutcome::Unrouted => {
                            (runtime.current_iter, "exhausted-unrouted".into())
                        }
                    };
                }
                LapDecision::NoReentry => {
                    return (runtime.current_iter, "no-reentry".into());
                }
            }
        }
    }

    #[test]
    fn full_run_exits_early_on_pass() {
        // FAIL, FAIL, PASS → exits at lap 3 to End, never reaching max.
        let (pipeline, region) = migrated_review_loop(5);
        let (laps, outcome) = run_region(&pipeline, &region, 5, &["FAIL", "FAIL", "PASS"]);
        assert_eq!(laps, 3);
        assert_eq!(outcome, "exit:end");
    }

    #[test]
    fn full_run_blocks_exhausted_unrouted_at_max_iter() {
        // Verdict never passes and no `iter >= max` exit is wired → at max_iter
        // the region blocks "exhausted — unrouted" (never a silent stall).
        let (pipeline, region) = migrated_review_loop(3);
        let (laps, outcome) = run_region(&pipeline, &region, 3, &["FAIL", "FAIL", "FAIL"]);
        assert_eq!(laps, 3);
        assert_eq!(outcome, "exhausted-unrouted");
    }

    // ── Collection region engine (ADR-0011 / #151) ──────────────────────────

    /// triage -> fixer (single-member collection over `issues`) -> merge -> end.
    /// The barrier edge leaves the region (fixer:fix -> merge:branches).
    fn collection_fanout_merge() -> (PipelineDef, LoopRegion) {
        let pipeline = PipelineDef {
            name: "cfm".into(),
            version: None,
            variables: Default::default(),
            nodes: vec![
                node("start", &[], &["user_prompt"]),
                node("triage", &["task"], &["plan"]),
                node("fixer", &["in"], &["fix"]),
                {
                    let mut m = node("merge", &["branches"], &["merged"]);
                    m.node_type = NodeType::Merge;
                    m
                },
                node("end", &["result"], &[]),
            ],
            edges: vec![
                edge("start", "user_prompt", "triage", "task"),
                edge("triage", "plan", "fixer", "in"),
                edge("fixer", "fix", "merge", "branches"),
                edge("merge", "merged", "end", "result"),
            ],
            loops: vec![],
            notes: Vec::new(),
            prompt_required: true,
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

    fn issues(names: &[&str]) -> std::collections::HashMap<String, serde_yaml::Value> {
        let mut fm = std::collections::HashMap::new();
        fm.insert(
            "issues".into(),
            serde_yaml::Value::Sequence(
                names
                    .iter()
                    .map(|n| serde_yaml::Value::String((*n).into()))
                    .collect(),
            ),
        );
        fm
    }

    #[test]
    fn collection_fans_out_one_lap_per_item_at_the_entry() {
        // A 3-item collection plans 3 laps; the entry is the single member
        // `fixer` (fed from outside the region). Each item becomes one parallel
        // lap of the entry.
        let (pipeline, region) = collection_fanout_merge();
        let fm = issues(&["a", "b", "c"]);
        let plan = collection_fanout(&pipeline, &region, &fm);
        assert_eq!(plan.total, 3);
        assert_eq!(plan.entry, "fixer");
        assert_eq!(plan.items.len(), 3);
    }

    #[test]
    fn collection_resolves_over_from_entering_frontmatter() {
        // `resolve_collection` reads the `over` field's list from the entering
        // artifact's frontmatter.
        let (_pipeline, region) = collection_fanout_merge();
        let fm = issues(&["x", "y"]);
        let items = resolve_collection(&region, &fm);
        assert_eq!(
            items,
            vec![
                serde_yaml::Value::String("x".into()),
                serde_yaml::Value::String("y".into()),
            ]
        );
    }

    #[test]
    fn empty_collection_fans_out_zero_items() {
        // An empty `issues: []` resolves to total 0 — no entry spawns; the caller
        // fires the barrier immediately.
        let (pipeline, region) = collection_fanout_merge();
        let mut fm = std::collections::HashMap::new();
        fm.insert("issues".into(), serde_yaml::Value::Sequence(vec![]));
        let plan = collection_fanout(&pipeline, &region, &fm);
        assert_eq!(plan.total, 0);
        assert!(plan.items.is_empty());
    }

    #[test]
    fn missing_over_field_is_an_empty_collection() {
        // A missing `over` field (sharp tool, ADR-0001): empty collection, no
        // error — the barrier fires immediately.
        let (pipeline, region) = collection_fanout_merge();
        let plan = collection_fanout(&pipeline, &region, &Default::default());
        assert_eq!(plan.total, 0);
    }

    #[test]
    fn barrier_is_reached_only_when_all_item_laps_complete() {
        // The barrier (outgoing edges fire once) is reached only when laps 1..=N
        // have all completed — not on the first item.
        let mut done: std::collections::HashSet<i64> = std::collections::HashSet::new();
        done.insert(1);
        assert!(!collection_barrier_reached(3, &done), "1/3 not barriered");
        done.insert(2);
        assert!(!collection_barrier_reached(3, &done), "2/3 not barriered");
        done.insert(3);
        assert!(collection_barrier_reached(3, &done), "3/3 barriered");
    }

    #[test]
    fn empty_collection_barrier_is_reached_immediately() {
        // total 0 ⇒ barrier vacuously reached, fires immediately with zero
        // item-artifacts.
        assert!(collection_barrier_reached(0, &Default::default()));
    }

    #[test]
    fn barrier_fires_once_into_the_merge_target() {
        // The region's single outgoing edge (fixer:fix -> merge:branches) leaves
        // the region: the barrier fires once into `merge`, preserving the
        // done -> Merge convergence (ADR-0006). The intra-region entering edge
        // (triage -> fixer) is NOT a barrier target.
        let (pipeline, region) = collection_fanout_merge();
        let targets = collection_barrier_targets(&pipeline, &region);
        assert_eq!(targets, vec!["merge".to_string()]);
    }
}
