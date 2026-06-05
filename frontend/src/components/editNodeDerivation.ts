import type { Node } from "@xyflow/react";
import type { NodeStatus, NodeType, PipelineDef, PortSide, RunState } from "../types";
import { runReachedEnd } from "./dagCanvasUtils";

export function statusForNode(
  nodeId: string,
  runState: RunState | null | undefined,
): NodeStatus {
  return runState?.nodes[nodeId]?.status ?? "pending";
}

/**
 * Whether a start/end marker should show the green "reached the end" cadre in
 * the inline run view (`EditCanvas`). Mirrors the `reached` flag DagCanvas's
 * `deriveNodes` set, so the intent of issue #105 survives in the view users
 * actually see. Only the start/end markers carry this — regular nodes always
 * report their own live status. It is gated on a live run: editing a
 * library/template pipeline (no run state) never colours the markers.
 */
export function markerReached(
  nodeType: NodeType,
  runState: RunState | null | undefined,
): boolean {
  if (nodeType !== "start" && nodeType !== "end") return false;
  return runState != null && runReachedEnd(runState.status);
}

export function deriveEditNodes(
  pipeline: PipelineDef,
  runState: RunState | null | undefined,
): Node[] {
  return pipeline.nodes.map((n, i) => {
    const status = statusForNode(n.id, runState);
    if (n.type === "switch") {
      return {
        id: n.id,
        type: "switch",
        position: {
          x: n.view?.x ?? 200,
          y: n.view?.y ?? 80 + i * 140,
        },
        data: {
          label: n.name ?? n.id,
          nodeId: n.id,
          status,
          branches: n.outputs.map((p) => ({
            name: p.name,
            side: p.side ?? "right",
            hasWhen: p.when != null,
          })),
          inputSide: n.inputs[0]?.side ?? "left",
        },
      };
    }
    if (n.type === "merge") {
      return {
        id: n.id,
        type: "merge",
        position: {
          x: n.view?.x ?? 200,
          y: n.view?.y ?? 80 + i * 140,
        },
        data: {
          label: n.name ?? n.id,
          nodeId: n.id,
          status,
          inputSide: n.inputs[0]?.side ?? "left",
          outputSide: n.outputs[0]?.side ?? "right",
        },
      };
    }
    if (n.type === "loop") {
      return {
        id: n.id,
        type: "loop",
        position: {
          x: n.view?.x ?? 200,
          y: n.view?.y ?? 80 + i * 140,
        },
        data: {
          label: n.name ?? n.id,
          nodeId: n.id,
          status,
          maxIter: n.max_iter ?? 5,
          ports: [
            ...n.inputs.map((p) => ({ name: p.name, kind: "input" as const, side: (p.side ?? "left") as PortSide })),
            ...n.outputs.map((p) => ({ name: p.name, kind: "output" as const, side: (p.side ?? "right") as PortSide })),
          ],
        },
      };
    }
    if (n.type === "for-each") {
      return {
        id: n.id,
        type: "foreach",
        position: {
          x: n.view?.x ?? 200,
          y: n.view?.y ?? 80 + i * 140,
        },
        data: {
          label: n.name ?? n.id,
          nodeId: n.id,
          status,
          ports: [
            ...n.inputs.map((p) => ({ name: p.name, kind: "input" as const, side: (p.side ?? "left") as PortSide })),
            ...n.outputs.map((p) => ({ name: p.name, kind: "output" as const, side: (p.side ?? "right") as PortSide })),
          ],
        },
      };
    }
    return {
      id: n.id,
      type: "edit",
      position: {
        x: n.view?.x ?? 200,
        y: n.view?.y ?? 80 + i * 140,
      },
      data: {
        label: n.name ?? n.id,
        nodeId: n.id,
        nodeType: n.type,
        status,
        reached: markerReached(n.type, runState),
        inputs: n.inputs.map((p) => ({ name: p.name, side: p.side ?? "left", description: p.description })),
        outputs: n.outputs.map((p) => ({ name: p.name, side: p.side ?? "right", description: p.description })),
        interactive: n.interactive,
      },
    };
  });
}
