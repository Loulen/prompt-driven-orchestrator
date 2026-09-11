/**
 * Run tree (#783 / #784) — the pure read model behind the Runs list's tree
 * rendering. See CONTEXT.md § "Orchestration récursive — arbre de runs".
 *
 * Everything that is NOT rendering lives here: building the tree from the flat
 * `GET /runs` list (a child hangs under its parent when the parent is listed;
 * an orphan — parent forgotten or absent — is a root), the aggregated child
 * counts over a whole subtree (four disjoint pills whose sum is the number of
 * descendants), the filter that keeps a parent as long as a descendant matches,
 * the ancestor walk that reveals a hidden run, and the flattening into the
 * visible row order (the basis for shift-range and select-all-visible).
 *
 * "Archived" is NOT a tree concept here: a child follows its parent into the
 * Archived section whatever its own status, so the caller splits on the ROOT's
 * status after `buildRunTree` and before `filterRunTree`.
 */
import type { RunListEntry } from "../types";
import { isLiveRun } from "../types";
import type { ChildCounts } from "./orchestration";

export interface RunTreeNode {
  run: RunListEntry;
  children: RunTreeNode[];
  /** Depth in the tree: 0 for a root. */
  depth: number;
  /**
   * The four pills, aggregated over EVERY descendant (not just direct
   * children) — always the real subtree, never the filtered view.
   */
  counts: ChildCounts;
}

/** The bucket a single run falls in: the four sets are disjoint by construction. */
export function runCountBucket(run: RunListEntry): keyof ChildCounts {
  if (run.status === "failed") return "failed";
  if (isLiveRun(run.status)) return run.stalled ? "stale" : "running";
  return "finished";
}

export function emptyCounts(): ChildCounts {
  return { finished: 0, failed: 0, stale: 0, running: 0 };
}

function addCounts(into: ChildCounts, from: ChildCounts): void {
  into.finished += from.finished;
  into.failed += from.failed;
  into.stale += from.stale;
  into.running += from.running;
}

/**
 * Build the forest from the flat list. Sibling order is the list's own order
 * (the daemon already sorts by start date), so children read like the list.
 * A run whose `parent_run_id` names a run that is NOT in the list is a root:
 * a forgotten parent must not swallow its children.
 *
 * Cycles cannot arise from real daemon data (a parent always predates its
 * child), but a defensive guard keeps a corrupted pair from recursing forever:
 * a run already placed is never placed twice.
 */
export function buildRunTree(runs: RunListEntry[]): RunTreeNode[] {
  const byId = new Map(runs.map((r) => [r.run_id, r]));
  const childrenOf = new Map<string, RunListEntry[]>();
  const roots: RunListEntry[] = [];
  for (const r of runs) {
    const pid = r.parent_run_id;
    if (pid && pid !== r.run_id && byId.has(pid)) {
      const list = childrenOf.get(pid) ?? [];
      list.push(r);
      childrenOf.set(pid, list);
    } else {
      roots.push(r);
    }
  }
  const placed = new Set<string>();
  const build = (run: RunListEntry, depth: number): RunTreeNode => {
    placed.add(run.run_id);
    const kids = (childrenOf.get(run.run_id) ?? []).filter((k) => !placed.has(k.run_id));
    const children = kids.map((k) => build(k, depth + 1));
    const counts = emptyCounts();
    for (const child of children) {
      counts[runCountBucket(child.run)] += 1;
      addCounts(counts, child.counts);
    }
    return { run, children, depth, counts };
  };
  const forest = roots.map((r) => build(r, 0));
  // A run caught in a cycle (never reachable from a root) is surfaced as a root
  // rather than dropped — a list must never lose a run.
  for (const r of runs) {
    if (!placed.has(r.run_id)) forest.push(build(r, 0));
  }
  return forest;
}

/**
 * Keep every node that matches OR has a matching descendant; a kept ancestor
 * keeps only its matching (or ancestor-of-matching) children. Counts are NOT
 * recomputed: the pills describe the real subtree, so a filtered parent still
 * says how many children it truly has.
 */
export function filterRunTree(
  nodes: RunTreeNode[],
  matches: (run: RunListEntry) => boolean,
): RunTreeNode[] {
  const out: RunTreeNode[] = [];
  for (const node of nodes) {
    const children = filterRunTree(node.children, matches);
    if (matches(node.run) || children.length > 0) {
      out.push({ ...node, children });
    }
  }
  return out;
}

/** Ids of every node that has at least one child (the rows with a chevron). */
export function parentIds(nodes: RunTreeNode[]): string[] {
  const out: string[] = [];
  const walk = (list: RunTreeNode[]) => {
    for (const n of list) {
      if (n.children.length > 0) {
        out.push(n.run.run_id);
        walk(n.children);
      }
    }
  };
  walk(nodes);
  return out;
}

/**
 * Ids of the nodes the filter kept ONLY as a path to a matching descendant
 * (they do not match themselves). These are the parents the filter opens.
 */
export function pathOnlyIds(
  nodes: RunTreeNode[],
  matches: (run: RunListEntry) => boolean,
): string[] {
  const out: string[] = [];
  const walk = (list: RunTreeNode[]) => {
    for (const n of list) {
      if (n.children.length > 0) {
        if (!matches(n.run)) out.push(n.run.run_id);
        walk(n.children);
      }
    }
  };
  walk(nodes);
  return out;
}

/**
 * The ancestors of `runId` in the flat list, nearest first — following
 * `parent_run_id` only through parents that ARE listed (an orphan has none).
 * Guarded against cycles.
 */
export function ancestorIds(runs: RunListEntry[], runId: string): string[] {
  const byId = new Map(runs.map((r) => [r.run_id, r]));
  const out: string[] = [];
  const seen = new Set<string>([runId]);
  let cur = byId.get(runId);
  while (cur?.parent_run_id && byId.has(cur.parent_run_id) && !seen.has(cur.parent_run_id)) {
    out.push(cur.parent_run_id);
    seen.add(cur.parent_run_id);
    cur = byId.get(cur.parent_run_id);
  }
  return out;
}

/** The root run of `runId`'s tree (itself when it is a root). */
export function rootOf(runs: RunListEntry[], runId: string): RunListEntry | undefined {
  const chain = ancestorIds(runs, runId);
  const rootId = chain.length > 0 ? chain[chain.length - 1] : runId;
  return runs.find((r) => r.run_id === rootId);
}

/**
 * Depth-first flattening in VISIBLE order: a node's children are included only
 * when `isExpanded(node)` says so. This is the order a shift-range spans and
 * Ctrl-A selects — a collapsed subtree is not "visible".
 */
export function flattenVisible(
  nodes: RunTreeNode[],
  isExpanded: (runId: string) => boolean,
): RunTreeNode[] {
  const out: RunTreeNode[] = [];
  const walk = (list: RunTreeNode[]) => {
    for (const n of list) {
      out.push(n);
      if (n.children.length > 0 && isExpanded(n.run.run_id)) walk(n.children);
    }
  };
  walk(nodes);
  return out;
}

/** Every run of the forest, depth-first, expanded or not (for "select all in group"). */
export function flattenAll(nodes: RunTreeNode[]): RunListEntry[] {
  return flattenVisible(nodes, () => true).map((n) => n.run);
}
