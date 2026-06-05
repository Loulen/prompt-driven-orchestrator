import type { Edge, Node } from "@xyflow/react";
import { MarkerType } from "@xyflow/react";
import type { NodeStatus, NodeType, PipelineDef, PortSide, RunState, RunStatus } from "../types";

/**
 * A run "reaches its end" when it terminates successfully (`completed`). At
 * that point the start/end marker nodes pick up the same green "done" signal
 * that standard nodes already show on completion. Failed/halted runs are not
 * treated as reached — the end node keeps its neutral "blocked" colour.
 */
export function runReachedEnd(status: RunStatus): boolean {
  return status === "completed";
}

export function statusForNode(
  nodeId: string,
  runState: RunState | null | undefined,
): NodeStatus {
  return runState?.nodes[nodeId]?.status ?? "pending";
}

/**
 * Whether a start/end marker should show the green "reached the end" cadre in
 * the inline run view (`EditCanvas`), preserving the intent of issue #105 in
 * the view users actually see. Only the start/end markers carry this — regular
 * nodes always report their own live status. It is gated on a live run: editing
 * a library/template pipeline (no run state) never colours the markers.
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

const OP_SYMBOLS: Record<string, string> = {
  eq: "=",
  neq: "!=",
  lt: "<",
  lte: "<=",
  gt: ">",
  gte: ">=",
};

/**
 * Renders a `when:` clause (ADR-0002 grammar) as a compact, human-readable pill
 * string for the canvas. Multiple predicates are joined with "and"; `in` /
 * `not_in` show a bracketed list. The shape mirrors the mechanical predicate
 * grammar exactly — no LLM-eval, no free expression (ADR-0011).
 */
export function formatWhenPill(when: Record<string, unknown>): string {
  const parts: string[] = [];
  for (const [field, predicate] of Object.entries(when)) {
    if (predicate == null || typeof predicate !== "object") {
      parts.push(field);
      continue;
    }
    for (const [op, value] of Object.entries(predicate as Record<string, unknown>)) {
      if (op === "in" || op === "not_in") {
        const list = Array.isArray(value) ? value.join(", ") : String(value);
        parts.push(`${field} ${op} [${list}]`);
      } else {
        const sym = OP_SYMBOLS[op] ?? op;
        parts.push(`${field} ${sym} ${String(value)}`);
      }
    }
  }
  return parts.join(" and ");
}

export interface EditEdgeData extends Record<string, unknown> {
  isConditional: boolean;
  isElse: boolean;
}

/**
 * Derives xyflow edges from a pipeline. Conditional edges (ADR-0011) carry an
 * always-visible condition pill at their midpoint: the rendered `when:` clause
 * for guarded edges, the literal "else" for fallback edges. The pill is the
 * edge's `label` (xyflow renders it at the midpoint, not gated on hover/select).
 * Unconditional edges carry no label.
 */
/**
 * Decodes the `pipeline.edges` index from a canvas edge id (`e-{index}`).
 * Returns null for ids that are not edge ids (node ids, malformed). This is the
 * inverse of the `id: \`e-${i}\`` assignment in {@link deriveEditEdges} and is
 * how an edge click resolves to the edge selection (#147).
 */
export function edgeIndexFromId(edgeId: string): number | null {
  const m = /^e-(\d+)$/.exec(edgeId);
  if (!m) return null;
  return Number(m[1]);
}

export function deriveEditEdges(pipeline: PipelineDef): Edge<EditEdgeData>[] {
  const endNodeId = pipeline.nodes.find((n) => n.type === "end")?.id;

  return pipeline.edges.map((e, i) => {
    const isEndEdge = endNodeId != null && e.target.node === endNodeId;
    const isElse = e.else === true;
    const hasWhen = e.when != null && Object.keys(e.when).length > 0;
    const isConditional = isElse || hasWhen;

    const isDashed = isEndEdge;
    const strokeColor = isDashed
      ? "var(--color-st-blocked, #f97316)"
      : isConditional
        ? "var(--color-acc)"
        : "var(--color-fg-4)";

    const label = isElse
      ? "else"
      : hasWhen
        ? formatWhenPill(e.when as Record<string, unknown>)
        : undefined;

    return {
      id: `e-${i}`,
      source: e.source.node,
      target: e.target.node,
      sourceHandle: e.source.port || null,
      targetHandle: e.target.port || null,
      type: "default",
      label,
      labelShowBg: isConditional,
      labelBgPadding: [6, 3] as [number, number],
      labelBgBorderRadius: 6,
      labelStyle: { fill: "var(--color-fg)", fontSize: 10, fontFamily: "var(--font-mono, monospace)" },
      labelBgStyle: { fill: "var(--color-bg-2, #1e1e1e)", stroke: strokeColor, strokeWidth: 1 },
      data: { isConditional, isElse },
      style: {
        stroke: strokeColor,
        strokeWidth: 1.5,
        strokeDasharray: isDashed ? "6 3" : undefined,
      },
      markerEnd: {
        type: MarkerType.ArrowClosed,
        color: strokeColor,
        width: 16,
        height: 16,
      },
    };
  });
}
