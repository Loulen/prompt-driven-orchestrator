/**
 * Path coherence of a Pipeline — the editor's MIRROR of the daemon's rule
 * (ADR-0081, CONTEXT.md « Pipeline non lançable »; daemon:
 * `crates/pdo-daemon/src/path_coherence.rs`, the only authority).
 *
 * A Pipeline is launchable iff, following EVERY edge (`when`/`else` edges and
 * loop re-entries are ordinary edges — coherence is static):
 *  1. `End` is reachable from `Start`;
 *  2. no Cul-de-sac: every node other than `End` leads to `End`;
 *  3. no Nœud orphelin: every node other than `Start` is reachable from `Start`.
 *
 * Run live on the edited graph (#1022) to feed the lint banner and the node
 * marks without saving. Held to the SAME fixture corpus as the daemon
 * (`crates/pdo-daemon/tests/fixtures/path-coherence/`): same defects, same
 * order, same sentences. Edges naming a node that does not exist are ignored
 * (dangling references are another check's business).
 */
import type { PathDefect } from "../types";

/** The minimal graph the rule reads — a `PipelineDef` fits. */
export interface CoherenceGraph {
  nodes: { id: string; name?: string | null; type: string }[];
  edges: { source: { node: string }; target: { node: string } }[];
}

const NO_PATH = "Not launchable: no path from Start to End.";
/** The prefix every defect sentence carries; New Run's reason line drops it. */
export const NOT_LAUNCHABLE_PREFIX = "Not launchable: ";

/** The exact daemon sentence for one defect (grilling Q18). */
export function defectMessage(kind: PathDefect["kind"], nodeName = ""): string {
  switch (kind) {
    case "no_path":
      return NO_PATH;
    case "dead_end":
      return `Not launchable: dead end — node "${nodeName}" never leads to End.`;
    case "orphan":
      return `Not launchable: orphan node — node "${nodeName}" is not reachable from Start.`;
  }
}

/** A node is named by its displayed name; a blank name falls back to its id. */
function displayName(node: { id: string; name?: string | null }): string {
  return node.name && node.name.trim() !== "" ? node.name : node.id;
}

function reachable(from: string, adjacency: Map<string, string[]>): Set<string> {
  const seen = new Set([from]);
  const queue = [from];
  while (queue.length > 0) {
    const node = queue.shift()!;
    for (const next of adjacency.get(node) ?? []) {
      if (!seen.has(next)) {
        seen.add(next);
        queue.push(next);
      }
    }
  }
  return seen;
}

/**
 * Every path-coherence defect of `graph`, in the daemon's order: « no path »
 * first, then every Cul-de-sac, then every Nœud orphelin — each group in node
 * order. Empty ⇔ launchable. A missing Start/End marker reads as « no path ».
 */
export function pathDefects(graph: CoherenceGraph): PathDefect[] {
  const start = graph.nodes.find((n) => n.type === "start");
  const end = graph.nodes.find((n) => n.type === "end");
  const ids = new Set(graph.nodes.map((n) => n.id));
  const forward = new Map<string, string[]>();
  const backward = new Map<string, string[]>();
  for (const edge of graph.edges) {
    const source = edge.source.node;
    const target = edge.target.node;
    if (!ids.has(source) || !ids.has(target)) continue;
    forward.set(source, [...(forward.get(source) ?? []), target]);
    backward.set(target, [...(backward.get(target) ?? []), source]);
  }
  const fromStart = start ? reachable(start.id, forward) : new Set<string>();
  const toEnd = end ? reachable(end.id, backward) : new Set<string>();

  const defects: PathDefect[] = [];
  if (!end || !fromStart.has(end.id)) {
    defects.push({ kind: "no_path", message: NO_PATH });
  }
  const work = graph.nodes.filter((n) => n.type !== "start" && n.type !== "end");
  const culprit = (kind: "dead_end" | "orphan", node: (typeof work)[number]): PathDefect => {
    const name = displayName(node);
    return { kind, node_id: node.id, node_name: name, message: defectMessage(kind, name) };
  };
  for (const node of work) if (!toEnd.has(node.id)) defects.push(culprit("dead_end", node));
  for (const node of work) if (!fromStart.has(node.id)) defects.push(culprit("orphan", node));
  return defects;
}

/** One mark line on a faulty node's card (design Q1 = B). */
export type NodeMarkKind = PathDefect["kind"];

/** The caption under a faulty node's name, one per defect. */
export const NODE_MARK_CAPTION: Record<NodeMarkKind, string> = {
  dead_end: "Dead end — never leads to End",
  orphan: "Orphan — not reachable from Start",
  no_path: "No path from Start to End",
};

/**
 * The marks each node carries, keyed by node id: a Cul-de-sac / Nœud orphelin
 * on its culprit (an isolated node carries both). « No path » names no node of
 * its own: it is pinned on Start ONLY when Start has no outgoing edge;
 * otherwise it stays banner-only.
 */
export function nodeMarks(graph: CoherenceGraph, defects: PathDefect[]): Map<string, NodeMarkKind[]> {
  const marks = new Map<string, NodeMarkKind[]>();
  const add = (id: string, kind: NodeMarkKind) => {
    const list = marks.get(id) ?? [];
    if (!list.includes(kind)) marks.set(id, [...list, kind]);
  };
  const ids = new Set(graph.nodes.map((n) => n.id));
  for (const defect of defects) {
    if (defect.kind === "no_path") {
      const start = graph.nodes.find((n) => n.type === "start");
      const wired = start
        ? graph.edges.some((e) => e.source.node === start.id && ids.has(e.target.node))
        : true;
      if (start && !wired) add(start.id, "no_path");
    } else if (defect.node_id && ids.has(defect.node_id)) {
      add(defect.node_id, defect.kind);
    }
  }
  return marks;
}
