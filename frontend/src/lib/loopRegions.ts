import type { EdgeDef, LoopRegion, NodeDef } from "../types";

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
