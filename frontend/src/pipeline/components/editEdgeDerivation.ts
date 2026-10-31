// The canvas side of a pipeline's edges: the xyflow edges, their condition
// pills, their draw order (#845) and the sides they leave and land on.
import type { Edge } from "@xyflow/react";
import { MarkerType } from "@xyflow/react";
import type { EdgeWaypoint } from "../../shared/generated/EdgeWaypoint";
import type { PortSide } from "../../shared/generated/PortSide";
import type { OrthogonalEdgeData } from "./OrthogonalEdge";
import { anchorHandleId, landsByDrop, rimHandleId } from "../../lib/anchorSide";
import { carriedPorts, declaredOutputs, missingCarriedPorts, primaryPort } from "../../lib/edgePorts";
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
 * a lands-by-drop notion (#168) and such a node never has one, so its handle sits
 * on the left whatever the persisted value says (a port has no side).
 */
export function resolveTargetGeometrySide(
  target: EditorPipelineDef["nodes"][number],
  targetSide: PortSide,
): PortSide {
  if (landsByDrop(target.type)) return targetSide;
  return "left";
}

/**
 * The side an incoming edge lands on, read from what the file persists.
 *
 * The anchor comes first: a drop always writes one, while `target_side` is
 * written only when it is not `left` — so a left drop on an End lives in
 * `target_anchor` alone. Absent both, the edge lands on `left`. A legacy End
 * whose `result` declared another side reaches here already converted: the
 * daemon's parse turned that side into `target_side`.
 */
export function resolveTargetSide(edge: EditorPipelineDef["edges"][number]): PortSide {
  return edge.target_anchor?.side ?? edge.target_side ?? "left";
}

/**
 * The side a wire leaves its source by: its `source_anchor` (#844), else
 * `right`. A legacy output declared `side: bottom|left|top` reaches here already
 * converted: the daemon's parse turned that side into a `source_anchor` without
 * offset, the middle of the side, where the per-output dot used to sit.
 */
export function resolveSourceSide(edge: EditorPipelineDef["edges"][number]): PortSide {
  return edge.source_anchor?.side ?? "right";
}

/** The condition pill's text: `else`, the formatted `when`, or none. */
function conditionLabel(e: EditorPipelineDef["edges"][number]): string | undefined {
  if (e.else === true) return "else";
  if (e.when != null && Object.keys(e.when).length > 0) return formatWhenPill(e.when as Record<string, unknown>);
  return undefined;
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

  // Pinned condition pills (#845), so an edge's unpinned pill keeps off the
  // ones the author placed on other edges.
  const pinnedPills = pipeline.edges.flatMap((e, i) => {
    const label = conditionLabel(e);
    return label && e.condition_label_pos ? [{ edgeIndex: i, x: e.condition_label_pos.x, y: e.condition_label_pos.y, label }] : [];
  });

  return pipeline.edges.map((e, i) => {
    const isEndEdge = endNodeId != null && e.target.node === endNodeId;
    const sourceSide = resolveSourceSide(e);
    const targetNode = pipeline.nodes.find((n) => n.id === e.target.node);
    // The persisted landing side (#168, #844). Only meaningful for a target that
    // lands by drop; declared/structural handles ignore it.
    const targetSide: PortSide = resolveTargetSide(e);
    const targetHandle = targetNode
      ? resolveTargetHandle(targetNode, e.target.port, targetSide)
      : e.target.port || null;
    // The side the wire actually ARRIVES on, which is not always the persisted
    // one: a target that keeps a declared handle
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
    const strokeColor = "var(--text-3)";

    const label = conditionLabel(e);

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
      // before #844 has no anchor and leaves by the right side.
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
        otherPinnedPills: pinnedPills.filter((p) => p.edgeIndex !== i).map(({ x, y, label }) => ({ x, y, label })),
        outputLabelSlot: labelSlots[i],
        belowNodes: e.below_nodes === true,
        // CORE-88: drawn « cassé » rather than as a wire that looks sound.
        missingPorts: missingCarriedPorts(pipeline, e),
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
