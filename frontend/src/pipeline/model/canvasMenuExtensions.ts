/**
 * What the extensions' part of the canvas's right-click menu hands over (#30):
 * the node or the edge under the pointer, and the pipeline of the tab.
 */

import type { RunState } from "../../shared/generated/RunState";
import type { CanvasItemContext, CanvasPipeline, EdgeEntity, NodeEntity } from "../../shared/extensions/contract";
import type { CanvasMenu } from "../components/CanvasContextMenu";
import type { EditorPipelineDef } from "./pipeline";

/** The extensions' part of the menu: its location, and what it hands over. */
export type CanvasMenuExtensions =
  | { location: "canvas/node/menu"; extra: Omit<CanvasItemContext<NodeEntity>, "location" | "run" | "project"> }
  | { location: "canvas/edge/menu"; extra: Omit<CanvasItemContext<EdgeEntity>, "location" | "run" | "project"> };

/** What the extensions' part of the menu hands over, for the element under the pointer. */
export function canvasMenuExtensions(
  menu: CanvasMenu,
  pipeline: EditorPipelineDef,
  pipelineId: string,
  run: RunState | null,
): CanvasMenuExtensions | null {
  const where: CanvasPipeline = { id: pipelineId, name: pipeline.name, run: run?.run_id ?? null };
  if (menu.type === "node") {
    const node = pipeline.nodes.find((n) => n.id === menu.id);
    if (!node) return null;
    const entity: NodeEntity = {
      kind: "node",
      id: node.id,
      name: node.name ?? null,
      type: node.type,
      status: run?.nodes[node.id]?.status ?? null,
      attributes: {},
    };
    return { location: "canvas/node/menu", extra: { entity, pipeline: where } };
  }
  if (menu.type === "edge" && menu.edgeIndex !== undefined) {
    const edge = pipeline.edges[menu.edgeIndex];
    if (!edge) return null;
    const sourcePort = edge.source.port ?? (edge.source.ports ?? []).join("|");
    const entity: EdgeEntity = {
      kind: "edge",
      id: `${edge.source.node}.${sourcePort}->${edge.target.node}.${edge.target.port}`,
      from: edge.source.node,
      to: edge.target.node,
      attributes: {},
    };
    return { location: "canvas/edge/menu", extra: { entity, pipeline: where } };
  }
  return null;
}
