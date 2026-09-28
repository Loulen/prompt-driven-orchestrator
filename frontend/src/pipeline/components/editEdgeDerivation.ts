// The canvas side of a pipeline's edges: the xyflow edges, their condition
// pills, their draw order (#845) and the sides they leave and land on.
import type { Edge } from "@xyflow/react";
import { MarkerType } from "@xyflow/react";
import type { EdgeWaypoint } from "../../shared/generated/EdgeWaypoint";
import type { PortSide } from "../../shared/generated/PortSide";
import type { OrthogonalEdgeData } from "./OrthogonalEdge";
import { anchorHandleId, landsByDrop, rimHandleId } from "../../lib/anchorSide";
import { carriedPorts, declaredOutputs, primaryPort } from "../../lib/edgePorts";
import { outputLabelSlots, resolveOutputLabels } from "../../lib/edgeLabels";
import type { EditorPipelineDef } from "../model/pipeline";

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

export type EditEdgeData = OrthogonalEdgeData;

/**
 * Per-edge draw order (#845). xyflow stacks its edge layers by `zIndex`, so an
 * edge above 1000 draws over the node cards (nodes render around 0-1000 in the
 * same stacking context) and one at 0 draws under them.
 *
 * The labels and segment handles follow suit ({@link EDGE_LABELS_ABOVE_NODES}
 * / {@link EDGE_LABELS_UNDER_NODES}, set per element in `OrthogonalEdge`): an
 * elevated edge would otherwise sit above its own labels and let its 14px
 * invisible hit-stroke swallow every pointer event aimed at a label or a
 * segment handle — silently, with nothing in the console to explain it — and
 * an edge sent under the cards would keep its labels and handles painted over
 * them.
 *
 * 1001 clears xyflow's `elevateNodesOnSelect` (+1000 on the selected card), and
 * deliberately only TIES with {@link REGION_CHROME_Z}: the node layer is painted
 * after the edge layer, so a loop region's header band keeps winning the tie and
 * stays clickable (#455) instead of being buried under a passing arrow.
 */
export const EDGE_ABOVE_NODES = 1001;
export const EDGE_UNDER_NODES = 0;
/** Labels and handles of an edge drawn above the cards: one notch over it. */
export const EDGE_LABELS_ABOVE_NODES = 1002;
/** Labels and handles of an edge drawn under the cards: level with it. */
export const EDGE_LABELS_UNDER_NODES = 0;

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

/**
 * Resolves the xyflow `targetHandle` id an incoming edge must use so the arrow
 * actually anchors to the node it lands on. xyflow drops an edge whose
 * `targetHandle` matches no rendered handle (`getEdgePosition` → error 008), so
 * the id here must mirror what the target node renders:
 *
 * - Structural nodes (merge) render an id'd target handle per declared input
 *   via `PortPill`, so the edge keeps its declared port name.
 * - Emergent work nodes (`agent`, ADR-0011 / #149) and the End marker (#840)
 *   render one body-covering target handle PER SIDE. The edge binds to the handle for
 *   its chosen `target_side` (#168) so the arrow anchors and routes from that
 *   side; absent a `target_side`, it binds to the left handle, reproducing the
 *   legacy left-anchored behaviour. This is keyed on node TYPE, not input count:
 *   a work node still carrying a vestigial declared `in` (the un-finished #149
 *   migration) is emergent all the same — keying on `inputs.length === 1`
 *   instead forced every such node's arrows to the left (#175).
 */
function resolveTargetHandle(
  target: EditorPipelineDef["nodes"][number],
  declaredPort: string,
  targetSide: PortSide | null | undefined,
): string | null {
  if (target.type === "merge") {
    return declaredPort || null;
  }
  // A node that pins the wire to a declared handle keeps binding to it.
  if (!landsByDrop(target.type)) {
    return target.inputs[0]?.name ?? declaredPort ?? null;
  }
  // A body that lands by drop: anchor on the chosen side.
  return anchorHandleId(targetSide ?? "left");
}

/**
 * The side the arrow physically arrives on — what the edge's geometry must use,
 * as opposed to the persisted `target_side`.
 *
 * They differ exactly where the target keeps a DECLARED handle: `target_side` is
 * a lands-by-drop notion (#168) and such a node never has one, so the persisted
 * value reads back as the `left` default while the handle sits wherever the port
 * declares (a merge's `branches` on its own side). Routing to `left` there lays
 * the perpendicular landing leg on a border the wire does not touch.
 */
export function resolveTargetGeometrySide(
  target: EditorPipelineDef["nodes"][number],
  targetSide: PortSide,
): PortSide {
  if (landsByDrop(target.type)) return targetSide;
  return target.inputs[0]?.side ?? "left";
}

/**
 * The side an incoming edge lands on, read from what the file persists.
 *
 * The anchor comes first: a drop always writes one, while `target_side` is
 * written only when it is not `left` — so a left drop on an End declared `top`
 * lives in `target_anchor` alone. Absent both, a work node lands on the legacy
 * `left`, and the End marker on its `result`'s DECLARED side: that is where every
 * End edge saved before #840 landed, so an existing pipeline reopens unchanged
 * (the fallback trap of a rim/anchor rework, cf. `resolveSourceSide`).
 */
export function resolveTargetSide(
  target: EditorPipelineDef["nodes"][number] | undefined,
  edge: EditorPipelineDef["edges"][number],
): PortSide {
  const persisted = edge.target_anchor?.side ?? edge.target_side;
  if (persisted) return persisted;
  if (target?.type === "end") return target.inputs[0]?.side ?? "left";
  return "left";
}

/**
 * The side a wire leaves its source by. A drawn edge (#844) says so itself in
 * `source_anchor`. An edge drawn before #844 has no anchor: it left from its
 * output's DECLARED side — where the per-output dot used to sit — so that is the
 * fallback, `right` only when the port declares none. Falling straight back to
 * `right` sent every legacy `side: bottom|left|top` output out the wrong border,
 * back through its own card and into the target from the opposite side (#844 FP
 * iter-2, blocking).
 */
export function resolveSourceSide(
  source: EditorPipelineDef["nodes"][number] | undefined,
  edge: EditorPipelineDef["edges"][number],
): PortSide {
  if (edge.source_anchor) return edge.source_anchor.side;
  const port = primaryPort(edge.source);
  const declared = source?.outputs.find((p) => p.name === port) ?? source?.outputs[0];
  return declared?.side ?? "right";
}

export function deriveEditEdges(pipeline: EditorPipelineDef): Edge<EditEdgeData>[] {
  const endNodeId = pipeline.nodes.find((n) => n.type === "end")?.id;

  // Output-label slots (#845): fan-out edges drawn from one departure point
  // number their labels across the group, so their tags do not stack.
  const labelSlots = outputLabelSlots(
    pipeline.edges.map((e) => ({
      departure: `${e.source.node}\u0000${primaryPort(e.source)}`,
      labelCount: resolveOutputLabels(e.show_output_labels, declaredOutputs(pipeline, e).length)
        ? carriedPorts(e.source).length
        : 0,
    })),
  );

  return pipeline.edges.map((e, i) => {
    const isEndEdge = endNodeId != null && e.target.node === endNodeId;
    const sourceSide = resolveSourceSide(
      pipeline.nodes.find((n) => n.id === e.source.node),
      e,
    );
    const targetNode = pipeline.nodes.find((n) => n.id === e.target.node);
    // The persisted landing side (#168, #844). Only meaningful for a target that
    // lands by drop; declared/structural handles ignore it.
    const targetSide: PortSide = resolveTargetSide(targetNode, e);
    const targetHandle = targetNode
      ? resolveTargetHandle(targetNode, e.target.port, targetSide)
      : e.target.port || null;
    // The side the wire actually ARRIVES on, which is not always the persisted
    // one: a target that keeps a declared handle (a merge's `branches`)
    // ignores `target_side` entirely and pins the arrow on its own
    // side. Handing the edge the persisted `left` there made it lay the landing
    // leg across a side the handle is not on — a wire entering the card from the
    // left to reach a pin on its top border (#844, FP finding 2).
    const geometrySide: PortSide = targetNode
      ? resolveTargetGeometrySide(targetNode, targetSide)
      : targetSide;
    const isElse = e.else === true;
    const hasWhen = e.when != null && Object.keys(e.when).length > 0;
    const isConditional = isElse || hasWhen;

    const isDashed = isEndEdge;
    // Edge stroke color is no longer keyed on type (#177): every edge renders
    // grey by default and the *selected* edge is recolored to pastel orange
    // dynamically in `OrthogonalEdge` (selection is the dominant visual signal,
    // PRD #143 — the prior conditional-green / end-orange tinting is dropped).
    // The dash pattern still distinguishes the end edge; that's a separate
    // signal carried by `isDashed`, untouched here.
    const strokeColor = "var(--color-fg-4)";

    const label = isElse
      ? "else"
      : hasWhen
        ? formatWhenPill(e.when as Record<string, unknown>)
        : undefined;

    // Orthogonal routing (#154): the custom edge computes its own right-angle
    // path (auto: pathfind around nodes; manual: through persisted waypoints)
    // and renders the condition pill + segment handles itself, so we hand it
    // the routing fields plus the styling it needs.
    const waypoints: EdgeWaypoint[] | null = e.waypoints ?? null;
    // Labels (#845). The toggle's default is derived from how many outputs the
    // SOURCE NODE DECLARES — not from how many this edge carries: an edge
    // carrying one of two outputs still needs to say which one.
    const ports = carriedPorts(e.source);
    const showOutputLabels = resolveOutputLabels(
      e.show_output_labels,
      declaredOutputs(pipeline, e).length,
    );
    return {
      id: `e-${i}`,
      source: e.source.node,
      target: e.target.node,
      // #844: the arrow binds to the RIM strip of the side it leaves by — there
      // is no per-output handle any more. xyflow needs the binding for
      // connectivity and for its own geometry only; the drawn departure POINT
      // comes from `source_anchor` inside the edge component. An edge drawn
      // before #844 has no anchor and leaves by its output's declared side.
      sourceHandle: rimHandleId(sourceSide),
      targetHandle,
      type: "orthogonal",
      // Draw order (#845): edges sit ABOVE the node cards by default, so an
      // arrow crossing a card stays readable; "Draw under nodes" drops this one
      // to 0. `elevateEdgesOnSelect` is deliberately NOT enabled on the canvas —
      // it would lift a selected edge back above the cards and silently undo the
      // switch for as long as the edge is selected.
      zIndex: e.below_nodes === true ? EDGE_UNDER_NODES : EDGE_ABOVE_NODES,
      data: {
        edgeIndex: i,
        mode: e.mode ?? null,
        waypoints,
        targetSide: geometrySide,
        sourceSide,
        sourceAnchor: e.source_anchor ?? null,
        targetAnchor: e.target_anchor ?? null,
        isConditional,
        isElse,
        label,
        strokeColor,
        dashed: isDashed,
        ports,
        showOutputLabels,
        outputLabelPos: e.output_label_pos ?? null,
        conditionLabelPos: e.condition_label_pos ?? null,
        outputLabelSlot: labelSlots[i],
        belowNodes: e.below_nodes === true,
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
