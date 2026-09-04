import type { NodeDef, NodeType } from "../types";

/**
 * `start` / `end` are structural markers, not nodes the user owns (#684): a
 * pipeline has exactly one of each, they carry no agent/model/prompt, and
 * their ports are the pipeline's contract. Every gesture that deletes,
 * duplicates or edits a node must ask this first.
 */
export function isStructuralMarker(node: Pick<NodeDef, "type"> | NodeType | null | undefined): boolean {
  const type = typeof node === "string" ? node : node?.type;
  return type === "start" || type === "end";
}

export type NodeInspectorKind =
  /** Full edit surface (agent / merge / script nodes), with the Run/Edit tabs. */
  | "node"
  /** `StartInspector`: the run's real input — only meaningful inside a run. */
  | "run-start"
  /** `EndInspector`: the run's termination reasons — only inside a run. */
  | "run-end"
  /** Read-only marker pane: a `start`/`end` selected outside a run (#684). */
  | "marker";

export interface NodeInspectorInputs {
  nodeType: NodeType | null;
  /** The active edit tab is a run tab (`scope === "run"`). */
  isEditingRun: boolean;
  /** The run carries `start_node` (resp. `end_node`) runtime info. */
  hasRunStart: boolean;
  hasRunEnd: boolean;
}

/**
 * Decide what the right pane shows for a selected node. Before #684 a marker
 * selected outside a run fell through to the generic `NodeInspector`, exposing
 * Save-to-library, Name, Agent/Model/Prompt editors and a *Delete port* button
 * on `user_prompt`. Markers never reach the generic editor now.
 */
export function resolveNodeInspector(input: NodeInspectorInputs): NodeInspectorKind {
  if (!isStructuralMarker(input.nodeType)) return "node";
  if (input.nodeType === "start" && input.isEditingRun && input.hasRunStart) return "run-start";
  if (input.nodeType === "end" && input.isEditingRun && input.hasRunEnd) return "run-end";
  return "marker";
}
