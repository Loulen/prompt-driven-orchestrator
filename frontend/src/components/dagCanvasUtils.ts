import type { PipelineDef, RunStatus } from "../types";

export const START_NODE_OFFSET_X_PX = 180;

/**
 * A run "reaches its end" when it terminates successfully (`completed`). At
 * that point the start/end marker nodes pick up the same green "done" signal
 * that standard nodes already show on completion. Failed/halted runs are not
 * treated as reached — the end node keeps its neutral "blocked" colour.
 */
export function runReachedEnd(status: RunStatus): boolean {
  return status === "completed";
}

export function canvasToYamlX(type: string | undefined, canvasX: number): number {
  return type === "start" || type === "end"
    ? canvasX
    : canvasX - START_NODE_OFFSET_X_PX;
}

export function withUpdatedNodeView(
  pipeline: PipelineDef,
  nodeId: string,
  x: number,
  y: number,
): PipelineDef | null {
  const idx = pipeline.nodes.findIndex((n) => n.id === nodeId);
  if (idx < 0) return null;
  const updated = pipeline.nodes.slice();
  updated[idx] = {
    ...updated[idx],
    view: { x: Math.round(x), y: Math.round(y) },
  };
  return { ...pipeline, nodes: updated };
}
