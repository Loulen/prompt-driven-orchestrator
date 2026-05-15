import type { PipelineDef } from "../types";

export const START_NODE_OFFSET_X_PX = 180;

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
