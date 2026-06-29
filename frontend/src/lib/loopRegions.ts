import type { EdgeDef, LoopRegion, NodeDef, PipelineDef } from "../types";

/**
 * Default iteration cap for an auto-materialized bounded region, so a drawn
 * cycle is never accidentally unbounded (ADR-0011 / #148). Mirrors the daemon's
 * `loop_region::DEFAULT_MAX_ITER`.
 */
export const DEFAULT_MAX_ITER = 5;

/**
 * Detects the cycles in a node/edge graph (ADR-0011 / #148). A *cycle* is a
 * strongly-connected component of >= 2 nodes, or a single node carrying a
 * self-edge. Members are ordered by their position in `nodes` for determinism;
 * the cycle list is ordered by each cycle's first member. Mirrors the daemon's
 * `graph_resolver::detect_cycles` so the editor and the engine agree on which
 * members a region covers.
 */
export function detectCycles(nodes: NodeDef[], edges: EdgeDef[]): string[][] {
  const order = new Map<string, number>();
  nodes.forEach((n, i) => order.set(n.id, i));

  const adj = new Map<string, string[]>();
  const hasSelfEdge = new Set<string>();
  for (const e of edges) {
    const s = e.source.node;
    const t = e.target.node;
    if (!order.has(s) || !order.has(t)) continue;
    if (s === t) hasSelfEdge.add(s);
    const list = adj.get(s);
    if (list) list.push(t);
    else adj.set(s, [t]);
  }

  // Tarjan's strongly-connected components.
  const index = new Map<string, number>();
  const lowlink = new Map<string, number>();
  const onStack = new Set<string>();
  const stack: string[] = [];
  let nextIndex = 0;
  const sccs: string[][] = [];

  const strongconnect = (v: string): void => {
    index.set(v, nextIndex);
    lowlink.set(v, nextIndex);
    nextIndex += 1;
    stack.push(v);
    onStack.add(v);

    for (const w of adj.get(v) ?? []) {
      if (!index.has(w)) {
        strongconnect(w);
        lowlink.set(v, Math.min(lowlink.get(v)!, lowlink.get(w)!));
      } else if (onStack.has(w)) {
        lowlink.set(v, Math.min(lowlink.get(v)!, index.get(w)!));
      }
    }

    if (lowlink.get(v) === index.get(v)) {
      const component: string[] = [];
      for (;;) {
        const w = stack.pop()!;
        onStack.delete(w);
        component.push(w);
        if (w === v) break;
      }
      sccs.push(component);
    }
  };

  for (const n of nodes) {
    if (!index.has(n.id)) strongconnect(n.id);
  }

  const positionOf = (id: string) => order.get(id) ?? Number.MAX_SAFE_INTEGER;

  const cycles = sccs
    .filter((comp) => comp.length >= 2 || (comp[0] !== undefined && hasSelfEdge.has(comp[0])))
    .map((comp) => [...comp].sort((x, y) => positionOf(x) - positionOf(y)));

  cycles.sort((c1, c2) => positionOf(c1[0]) - positionOf(c2[0]));
  return cycles;
}

/**
 * A short, deterministic region id derived from the sorted member ids, prefixed
 * `loop-`. Mirrors the daemon's `loop_region::generated_region_id` (FNV-1a) so a
 * region auto-materialized in the editor keeps the same id the engine would
 * derive on the same member set.
 */
export function generatedRegionId(members: string[]): string {
  const sorted = [...members].sort();
  // FNV-1a over the sorted members, separated by 0x2f. BigInt keeps the 64-bit
  // arithmetic exact (Number would lose precision past 2^53).
  let hash = 0xcbf29ce484222325n;
  const prime = 0x100000001b3n;
  const mask = 0xffffffffffffffffn;
  for (const m of sorted) {
    for (let i = 0; i < m.length; i++) {
      hash = (hash ^ BigInt(m.charCodeAt(i) & 0xff)) & mask;
      hash = (hash * prime) & mask;
    }
    hash = (hash ^ 0x2fn) & mask;
  }
  // Rust formats this with `{hash:08x}` — hex, zero-padded to a *minimum* width
  // of 8 (a full 64-bit hash prints all 16 digits; only small hashes get
  // padded). `padStart(8, "0")` is the exact equivalent (never truncates).
  return `loop-${hash.toString(16).padStart(8, "0")}`;
}

/**
 * Returns the bounded regions that should be auto-materialized for cycles not
 * already covered by an existing `loops:` entry (ADR-0011 / #148, #166). A cycle
 * is "covered" when an existing region's member set is identical to it. Mirrors
 * the daemon's `loop_region::materialize_missing_regions`.
 */
export function materializeMissingRegions(
  nodes: NodeDef[],
  edges: EdgeDef[],
  existing: LoopRegion[] = [],
): LoopRegion[] {
  const covered = existing.map((r) => new Set(r.members));
  const sameSet = (a: Set<string>, b: string[]) =>
    a.size === b.length && b.every((m) => a.has(m));

  const out: LoopRegion[] = [];
  for (const cycle of detectCycles(nodes, edges)) {
    if (covered.some((c) => sameSet(c, cycle))) continue;
    out.push({
      id: generatedRegionId(cycle),
      kind: "bounded",
      members: cycle,
      max_iter: DEFAULT_MAX_ITER,
    });
  }
  return out;
}

/**
 * True when a bounded region's members still close a cycle under the given
 * edges (ADR-0011 / #150). A region "closes a cycle" when `detectCycles` finds a
 * cycle whose members are all members of the region — the topological signature
 * the region was materialized from. Mirrors the daemon's
 * `loop_region::region_has_cycle`.
 */
function regionHasCycle(region: LoopRegion, nodes: NodeDef[], edges: EdgeDef[]): boolean {
  const memberSet = new Set(region.members);
  return detectCycles(nodes, edges).some((cycle) => cycle.every((m) => memberSet.has(m)));
}

/**
 * Returns the ids of the `bounded` regions that would be **destroyed** by
 * removing the edge at `edgeIndex` (ADR-0011 / #150). A region is destroyed when
 * it currently closes a cycle but, with that edge gone, its members no longer
 * close any cycle — i.e. the deleted edge was the region's **last** cycle.
 * Deleting an edge while another cycle still closes the region leaves the loop
 * intact (not returned). `collection` regions have no topological cycle to lose
 * and are never returned. Mirrors the daemon's
 * `loop_region::regions_destroyed_by_edge_removal`.
 *
 * This is the destroy-vs-keep decision behind the confirmation popup: when the
 * list is non-empty, deleting the edge pops "this will destroy loop <id>"; on
 * confirm the caller removes each returned region's `loops:` entry (its bound
 * and iteration state go with it). An empty list pops nothing.
 */
export function regionsDestroyedByEdgeRemoval(
  pipeline: PipelineDef,
  edgeIndex: number,
): string[] {
  const edges = pipeline.edges;
  if (edgeIndex < 0 || edgeIndex >= edges.length) return [];
  const without = edges.filter((_, i) => i !== edgeIndex);
  return (pipeline.loops ?? [])
    .filter((region) => region.kind === "bounded")
    .filter(
      (region) =>
        regionHasCycle(region, pipeline.nodes, edges) &&
        !regionHasCycle(region, pipeline.nodes, without),
    )
    .map((region) => region.id);
}

/**
 * Reconciles the `loops:` regions against the current node set after a node has
 * been deleted (ADR-0011 / #150 / #173). Deleting a node also drops every edge
 * that referenced it — which can take a bounded region's **last** cycle — and
 * always leaves the deleted id dangling in any region's `members`. This rebuilds
 * the region list so it can never name a node absent from the graph nor keep an
 * orphan (cycle-less) bounded region:
 *
 *  - every region's `members` is pruned to nodes still present (no ghost ids —
 *    a loop never lists a node that doesn't exist);
 *  - a `bounded` region that no longer closes a cycle is dropped (its bound and
 *    iteration state go with the deleted node — the same destroy-on-last-cycle
 *    rule the edge path applies in `regionsDestroyedByEdgeRemoval`);
 *  - a region left with no present members is dropped.
 *
 * `pipeline` is the graph AFTER the node and its edges have been removed. A
 * `collection` region is born by gesture, not topology (#151), so it is kept on
 * its remaining members (only an emptied-out collection is dropped).
 */
export function reconcileLoopRegions(pipeline: PipelineDef): LoopRegion[] {
  const present = new Set(pipeline.nodes.map((n) => n.id));
  const out: LoopRegion[] = [];
  for (const region of pipeline.loops ?? []) {
    const members = region.members.filter((m) => present.has(m));
    if (members.length === 0) continue;
    const pruned =
      members.length === region.members.length ? region : { ...region, members };
    if (
      pruned.kind === "bounded" &&
      !regionHasCycle(pruned, pipeline.nodes, pipeline.edges)
    ) {
      continue;
    }
    out.push(pruned);
  }
  return out;
}

/**
 * One advisory fan-out nudge. The `id` is a stable, rename-proof dismiss
 * identity (#268): `fanout:<targetNodeId>`, keyed on the immutable target node
 * **id** rather than the display-name `message` (which changes on rename). It is
 * 1:1 with the rendered row (deduped by target below), so a dismiss never
 * collides with another nudge.
 */
export interface FanoutNudge {
  id: string;
  message: string;
}

/**
 * Info-only nudges (ADR-0001 sharp tool, never auto-wraps) suggesting a
 * collection fan-out (#151): when a `list`-typed output port feeds a downstream
 * node that is NOT already a member of a collection region, offer the explicit
 * "fan out over a collection" gesture. The nudge never blocks and never wraps
 * automatically — a `collection` region is born only by the user's explicit
 * gesture (unlike a `bounded` region, which auto-materializes on a drawn cycle).
 *
 * Each nudge carries a stable `id` (`fanout:<targetNodeId>`) so a user can
 * dismiss it persistently (#268). The advice is target-scoped ("this target
 * should fan out"), so the target id is the correct dismiss granularity.
 */
export function collectionFanoutNudges(pipeline: PipelineDef): FanoutNudge[] {
  const byId = new Map(pipeline.nodes.map((n) => [n.id, n]));
  const collectionMembers = new Set<string>();
  for (const region of pipeline.loops ?? []) {
    if (region.kind === "collection") {
      for (const m of region.members) collectionMembers.add(m);
    }
  }

  const nudges: FanoutNudge[] = [];
  const seenTargets = new Set<string>();
  for (const edge of pipeline.edges) {
    const src = byId.get(edge.source.node);
    if (!src) continue;
    const port = src.outputs.find((p) => p.name === edge.source.port);
    // The output is "list-typed" when any of its declared frontmatter fields is
    // a `list` (the frontmatter map is keyed by field name, not port name).
    const isList = Object.values(port?.frontmatter ?? {}).some((f) => f.type === "list");
    if (!isList) continue;
    const target = edge.target.node;
    if (collectionMembers.has(target)) continue; // already fanned out
    if (seenTargets.has(target)) continue;
    seenTargets.add(target);
    const targetNode = byId.get(target);
    nudges.push({
      id: `fanout:${target}`,
      message:
        `"${src.name ?? src.id}" emits a list into "${targetNode?.name ?? target}". ` +
        `Select the member(s) and choose "fan out over a collection" to run one lap per item.`,
    });
  }
  return nudges;
}
