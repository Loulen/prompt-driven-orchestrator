/**
 * The words of a change to a pipeline, as the Undo tooltip names it (« Undo
 * your last change: rename output spec »).
 */

import type { EditorNodeDef, EditorPipelineDef } from "./pipeline";
import { canonical } from "./draftDelta";

type Json = Parameters<typeof canonical>[0];

function same(a: unknown, b: unknown): boolean {
  return canonical(a as Json) === canonical(b as Json);
}

function nodeName(node: EditorNodeDef): string {
  return node.name || node.id;
}

function portChange(kind: "input" | "output", before: { name: string }[], after: { name: string }[]): string {
  if (after.length > before.length) return `add ${kind} ${after[after.length - 1].name}`;
  if (after.length < before.length) {
    const gone = before.find((p) => !after.some((q) => q.name === p.name));
    return `delete ${kind} ${gone?.name ?? ""}`.trim();
  }
  const renamed = after.find((p, i) => p.name !== before[i].name);
  if (renamed) return `rename ${kind} ${renamed.name}`;
  const edited = after.find((p, i) => !same(p, before[i]));
  return `edit ${kind} ${edited?.name ?? ""}`.trim();
}

function describeNode(before: EditorNodeDef, after: EditorNodeDef): string {
  const changed = Object.keys({ ...before, ...after }).filter(
    (key) => !same(before[key as keyof EditorNodeDef], after[key as keyof EditorNodeDef]),
  );
  if (changed.length === 1 && changed[0] === "view") return `move ${nodeName(after)}`;
  if (changed.includes("outputs")) return portChange("output", before.outputs, after.outputs);
  if (changed.includes("inputs")) return portChange("input", before.inputs, after.inputs);
  if (changed.length === 1 && changed[0] === "name") return `rename ${nodeName(before)} to ${nodeName(after)}`;
  return `edit ${changed[0] ?? "node"} of ${nodeName(after)}`;
}

/** Where an edge is drawn (its route), apart from what it is. */
const ROUTE_KEYS = ["mode", "waypoints", "target_side", "source_anchor", "target_anchor"];
/** Where an edge's labels sit. */
const LABEL_KEYS = ["output_label_pos", "condition_label_pos", "show_output_labels"];

function edgeChange(before: EditorPipelineDef["edges"], after: EditorPipelineDef["edges"]): string {
  const keys = new Set<string>();
  after.forEach((edge, i) => {
    const old = (before[i] ?? {}) as Record<string, unknown>;
    const next = edge as Record<string, unknown>;
    for (const key of Object.keys({ ...old, ...next })) if (!same(old[key], next[key])) keys.add(key);
  });
  const only = (group: string[]) => [...keys].every((key) => group.includes(key));
  if (only(ROUTE_KEYS)) return "reshape edge";
  if (only(LABEL_KEYS)) return "move edge label";
  return "edit edge";
}

/** What changed from `before` to `after`, in a few words; `null` when nothing did. */
export function describeChange(before: EditorPipelineDef, after: EditorPipelineDef): string | null {
  const old = new Map(before.nodes.map((n) => [n.id, n]));
  const next = new Map(after.nodes.map((n) => [n.id, n]));
  const added = after.nodes.filter((n) => !old.has(n.id));
  const removed = before.nodes.filter((n) => !next.has(n.id));
  if (added.length > 0) return added.length === 1 ? `add node ${nodeName(added[0])}` : `add ${added.length} nodes`;
  if (removed.length > 0) {
    return removed.length === 1 ? `delete node ${nodeName(removed[0])}` : `delete ${removed.length} nodes`;
  }
  const changed = after.nodes.filter((n) => !same(n, old.get(n.id)));
  if (changed.length > 1 && changed.every((n) => same({ ...n, view: null }, { ...old.get(n.id), view: null }))) {
    return `move ${changed.length} nodes`;
  }
  if (changed.length > 0) return describeNode(old.get(changed[0].id)!, changed[0]);
  if (after.edges.length > before.edges.length) return "add edge";
  if (after.edges.length < before.edges.length) return "delete edge";
  if (!same(after.edges, before.edges)) return edgeChange(before.edges, after.edges);
  if (!same(after.notes, before.notes)) return "edit note";
  if (!same(after.loops, before.loops)) return "edit loop";
  const field = Object.keys({ ...before, ...after }).find(
    (key) => !same(before[key as keyof EditorPipelineDef], after[key as keyof EditorPipelineDef]),
  );
  return field ? `edit pipeline ${field}` : null;
}
