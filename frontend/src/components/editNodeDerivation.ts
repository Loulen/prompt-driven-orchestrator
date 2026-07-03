import type { Edge, Node } from "@xyflow/react";
import { MarkerType } from "@xyflow/react";
import type { EdgeWaypoint, LoopRegion, NodeStatus, NodeType, PipelineDef, PortSide, RunState, RunStatus } from "../types";
import type { OrthogonalEdgeData } from "./OrthogonalEdge";
import { anchorHandleId, isEmergentInputNode } from "../lib/anchorSide";

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
  // A single-member loop region renders as a compact badge on the member's card
  // instead of a box (ADR-0011 / #148, #151; the LoopRegion type's "compact
  // badge (1 member)"). A `collection` reads `⇉ <fan-out>`; a single-member
  // `bounded` region (a self-loop, or a multi-member loop reduced to one present
  // member, e.g. after a member is deleted — #173) reads `↻ <counter>`,
  // mirroring the box header — so a one-member loop is never invisible (it draws
  // no box). Keyed by member id; the kind drives the glyph and the title.
  const loopBadgeByMember = new Map<string, { text: string; kind: LoopRegion["kind"] }>();
  for (const region of deriveLoopRegions(pipeline, runState)) {
    if (region.badgeMemberId == null) continue;
    const symbol = region.kind === "collection" ? "⇉" : "↻";
    loopBadgeByMember.set(region.badgeMemberId, {
      text: `${symbol} ${region.counterText}`,
      kind: region.kind,
    });
  }
  return pipeline.nodes.map((n, i) => {
    const status = statusForNode(n.id, runState);
    const loopBadge = loopBadgeByMember.get(n.id);
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
        // Input images uploaded with the run surface on the start marker only
        // (issue #145). They come from the projected run state, so template
        // editing (no run state) leaves the start node image-free.
        inputImages:
          n.type === "start" ? (runState?.start_node?.input_images ?? []) : undefined,
        inputs: n.inputs.map((p) => ({ name: p.name, side: p.side ?? "left", description: p.description })),
        outputs: n.outputs.map((p) => ({ name: p.name, side: p.side ?? "right", description: p.description })),
        interactive: n.interactive,
        // Compact badge for the single member of a loop region: `⇉ ...` for a
        // collection (#151) or `↻ ...` for a single-member bounded loop (#173).
        // Absent on non-member nodes and multi-member regions (boxed instead).
        loopBadge,
      },
    };
  });
}

/**
 * Layout + label for a bounded loop region (ADR-0011 / #148) rendered on the
 * canvas. The region is the named `loops:` entry — NOT a node. A region with
 * >= 2 members renders as a translucent box enclosing its members; a single-
 * member region renders as a compact badge on that member's card. The
 * `counterText` reads `max N` before a run and `i/N` once a run is live (where
 * `i` is the region-wide iteration, taken as the max `iter` across the member
 * nodes — the daemon stamps every member with the region iter for the lap).
 */
export interface LoopRegionLayout {
  id: string;
  kind: LoopRegion["kind"];
  /** Region members that actually exist as nodes, in pipeline order. */
  memberIds: string[];
  /**
   * `↻` counter text, e.g. `max 3` (idle) or `2/3` (running). Rendered read-only
   * on the canvas; the raw bound is edited in the RegionInspector, not inline.
   */
  counterText: string;
  /** True once the region has reached `max_iter` on a live run. */
  exhausted: boolean;
  /** Translucent-box geometry. Present iff `kind` renders as a box (>= 2 members). */
  box: { x: number; y: number; width: number; height: number } | null;
  /** Single-member badge anchor (the member's id). Present iff exactly 1 member. */
  badgeMemberId: string | null;
}

// Approximate slim-card footprint (px) used to bound the region box around its
// members. Cards auto-size; this is the padding-inclusive envelope the box
// must clear. Mirrors the `minWidth: 160` slim card in EditCanvas.
const CARD_W = 180;
const CARD_H = 54;
// Inset of the translucent box around the member extent. Leaves room for the
// `↻ X/Y` header pinned to the box's top edge (see refonte.css .rf-region-head).
const REGION_PAD = 26;
const REGION_PAD_TOP = 30;

function regionMaxIterText(maxIter: LoopRegion["max_iter"]): string {
  if (maxIter == null) return "∞";
  // A `$var` reference (string) is shown verbatim; a number is shown as-is.
  return String(maxIter);
}

/**
 * Derives the on-canvas layout for every bounded loop region in the pipeline.
 * Regions whose members are all missing (e.g. mid-edit deletion) are dropped.
 */
export function deriveLoopRegions(
  pipeline: PipelineDef,
  runState: RunState | null | undefined,
): LoopRegionLayout[] {
  const regions = pipeline.loops ?? [];
  const byId = new Map(pipeline.nodes.map((n) => [n.id, n]));
  // A present run state means the region has executed and member iters are
  // meaningful (`i/N`); template editing passes `null` and renders `max N`.
  const live = runState != null;

  const layouts: LoopRegionLayout[] = [];
  for (const region of regions) {
    const members = region.members
      .map((id) => byId.get(id))
      .filter((n): n is NonNullable<typeof n> => n != null);
    if (members.length === 0) continue;

    const maxText = regionMaxIterText(region.max_iter);
    // Region-wide current iter: the daemon stamps each member node with the
    // region's iteration, so the live lap is the max iter across members.
    const currentIter = live
      ? members.reduce(
          (max, n) => Math.max(max, runState?.nodes[n.id]?.iter ?? 0),
          0,
        )
      : 0;
    const maxNum =
      typeof region.max_iter === "number" ? region.max_iter : null;
    // A collection region (#151) never "exhausts" — the lap count is the
    // collection size, not a bounded cap. Its counter reads the fan-out (`N
    // items`), not a `i/N` loop counter; idle shows the `over` driver.
    const isCollection = region.kind === "collection";
    const exhausted =
      !isCollection && live && maxNum != null && currentIter >= maxNum;
    const counterText = isCollection
      ? live
        ? `${currentIter} items`
        : region.over
          ? `over ${region.over}`
          : "items"
      : live
        ? `${currentIter}/${maxText}`
        : `max ${maxText}`;

    if (members.length === 1) {
      layouts.push({
        id: region.id,
        kind: region.kind,
        memberIds: members.map((n) => n.id),
        counterText,
        exhausted,
        box: null,
        badgeMemberId: members[0].id,
      });
      continue;
    }

    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (const n of members) {
      const x = n.view?.x ?? 200;
      const y = n.view?.y ?? 200;
      minX = Math.min(minX, x);
      minY = Math.min(minY, y);
      maxX = Math.max(maxX, x + CARD_W);
      maxY = Math.max(maxY, y + CARD_H);
    }
    layouts.push({
      id: region.id,
      kind: region.kind,
      memberIds: members.map((n) => n.id),
      counterText,
      exhausted,
      badgeMemberId: null,
      box: {
        x: minX - REGION_PAD,
        y: minY - REGION_PAD_TOP,
        width: maxX - minX + REGION_PAD * 2,
        height: maxY - minY + REGION_PAD_TOP + REGION_PAD,
      },
    });
  }
  return layouts;
}

/**
 * Builds the decorative xyflow `loopRegion` nodes that back each box-form
 * bounded region (ADR-0011 / #148). One node per `>= 2`-member region; single-
 * member regions render as a badge on the member card (no box) and produce no
 * node here.
 *
 * The region box sits BEHIND the member cards and must never intercept pointer
 * events: an edge whose path crosses the box has to stay clickable/selectable
 * (#167). xyflow gives every node wrapper `pointer-events: all` whenever the
 * canvas registers node mouse handlers (it does, for the drag-highlight), so
 * pinning the inner div to `pointer-events: none` is not enough — the wrapper
 * still swallows the click. We override the wrapper's pointer-events to `none`
 * via the node's own `style` (xyflow spreads `node.style` AFTER its own
 * `pointerEvents`, so this wins without `!important`). The region header keeps
 * its own `pointer-events: auto` and stays clickable as a descendant.
 */
export function buildLoopRegionNodes(
  pipeline: PipelineDef,
  runState: RunState | null | undefined,
): Node[] {
  return deriveLoopRegions(pipeline, runState)
    .filter((r) => r.box != null)
    .map((r) => ({
      id: `region-${r.id}`,
      type: "loopRegion",
      position: { x: r.box!.x, y: r.box!.y },
      data: {
        regionId: r.id,
        kind: r.kind,
        counterText: r.counterText,
        exhausted: r.exhausted,
        // The live run id (or null in a template view) so an exhausted-unrouted
        // region can offer "route from manager" (ADR-0011 / #152).
        runId: runState?.run_id ?? null,
        width: r.box!.width,
        height: r.box!.height,
      },
      draggable: false,
      selectable: false,
      connectable: false,
      focusable: false,
      zIndex: 0,
      // `pointerEvents: "none"` defeats xyflow's wrapper-level `all` so edges
      // under the box remain clickable (#167); `zIndex: 0` keeps the box behind
      // the member cards.
      style: { zIndex: 0, pointerEvents: "none" },
    }));
}

/**
 * Builds the xyflow `note` nodes for every inert canvas note (#307 / ADR-0018).
 * A note is draggable and selectable but NOT connectable — it renders no handle,
 * so no edge can ever attach. Missing `view` falls back to a default spot, like
 * a node with no persisted position. Notes are a canvas concept, never part of
 * `pipeline.nodes`; they carry no run status and never colour.
 */
export function buildNoteNodes(pipeline: PipelineDef): Node[] {
  return (pipeline.notes ?? []).map((note, i) => ({
    id: note.id,
    type: "note",
    position: {
      x: note.view?.x ?? 240,
      y: note.view?.y ?? 80 + i * 120,
    },
    data: { noteId: note.id, content: note.content },
    draggable: true,
    selectable: true,
    connectable: false,
  }));
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

export type EditEdgeData = OrthogonalEdgeData;

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
 * - Declared-port `edit` nodes (the End node's `result`) keep that declared,
 *   side-fixed handle — those ports are unaffected by anchoring (#168).
 * - Emergent work nodes (`doc-only` / `code-mutating`, ADR-0011 / #149) render
 *   one body-covering target handle PER SIDE. The edge binds to the handle for
 *   its chosen `target_side` (#168) so the arrow anchors and routes from that
 *   side; absent a `target_side`, it binds to the left handle, reproducing the
 *   legacy left-anchored behaviour. This is keyed on node TYPE, not input count:
 *   a work node still carrying a vestigial declared `in` (the un-finished #149
 *   migration) is emergent all the same — keying on `inputs.length === 1`
 *   instead forced every such node's arrows to the left (#175).
 */
function resolveTargetHandle(
  target: PipelineDef["nodes"][number],
  declaredPort: string,
  targetSide: PortSide | null | undefined,
): string | null {
  if (target.type === "merge") {
    return declaredPort || null;
  }
  // Declared-port nodes (End's `result`) keep their declared, fixed-side handle;
  // they are unaffected by drop-position anchoring.
  if (!isEmergentInputNode(target.type)) {
    return target.inputs[0]?.name ?? declaredPort ?? null;
  }
  // Emergent work-node body: anchor on the chosen side (default left = legacy).
  return anchorHandleId(targetSide ?? "left");
}

export function deriveEditEdges(pipeline: PipelineDef): Edge<EditEdgeData>[] {
  const endNodeId = pipeline.nodes.find((n) => n.type === "end")?.id;

  return pipeline.edges.map((e, i) => {
    const isEndEdge = endNodeId != null && e.target.node === endNodeId;
    const targetNode = pipeline.nodes.find((n) => n.id === e.target.node);
    // The persisted anchor side (#168). Only meaningful for an emergent body
    // target; declared/structural handles ignore it. Defaults to `left` so an
    // un-anchored edge keeps the legacy left arrival.
    const targetSide: PortSide = e.target_side ?? "left";
    const targetHandle = targetNode
      ? resolveTargetHandle(targetNode, e.target.port, targetSide)
      : e.target.port || null;
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
    return {
      id: `e-${i}`,
      source: e.source.node,
      target: e.target.node,
      sourceHandle: e.source.port || null,
      targetHandle,
      type: "orthogonal",
      data: {
        edgeIndex: i,
        mode: e.mode ?? null,
        waypoints,
        targetSide,
        isConditional,
        isElse,
        label,
        strokeColor,
        dashed: isDashed,
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
