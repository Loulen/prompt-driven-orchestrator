// The canvas side of a pipeline's bounded loop regions (ADR-0011 / #148): the
// layout and label of each region, and the xyflow nodes that draw it.
import type { Node } from "@xyflow/react";
import type { LoopRegion } from "../../shared/generated/LoopRegion";
import type { RunState } from "../../shared/generated/RunState";
import type { EditorPipelineDef } from "../model/pipeline";

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
  pipeline: EditorPipelineDef,
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
    // collection size, not a bounded cap. Idle shows the `over` driver.
    const isCollection = region.kind === "collection";
    const exhausted =
      !isCollection && live && maxNum != null && currentIter >= maxNum;
    // #453: on a live run the collection badge reads `laps/total`, where the
    // total is the region's RESOLVED size (`collection_states[id].total_items`),
    // not `max(iter)` across members. `max(iter)` is the last lap *reached*, so
    // a region wedged at lap 1 of 2 rendered exactly like a finished 1-item
    // region — the canvas contradicted nothing and the failure was invisible.
    // Falls back to the bare lap count when the region has no projected state
    // yet (the fan-out has not resolved its `over` list).
    const totalItems = live
      ? (runState?.collection_states?.[region.id]?.total_items ?? null)
      : null;
    const counterText = isCollection
      ? live
        ? totalItems != null
          ? `${currentIter}/${totalItems} items`
          : `${currentIter} items`
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
 * xyflow's `elevateNodesOnSelect` (on by default) adds `SELECTED_NODE_Z = 1000`
 * to whichever node is selected. Region chrome has to outrank even that: a
 * selected card overlapping the header is precisely when a user reaches for the
 * region, and anything ≤ 1000 would leave the header unreachable right then.
 */
const REGION_CHROME_Z = 1001;

/**
 * Builds the decorative xyflow `loopRegion` nodes that back each box-form
 * bounded region (ADR-0011 / #148). TWO nodes per `>= 2`-member region; single-
 * member regions render as a badge on the member card (no box) and produce
 * neither.
 *
 * Neither node may intercept pointer events at the wrapper level: an edge whose
 * path crosses the box has to stay clickable/selectable (#167). xyflow gives
 * every node wrapper `pointer-events: all` whenever the canvas registers node
 * mouse handlers (it does, for the drag-highlight), so pinning the inner div to
 * `pointer-events: none` is not enough — the wrapper still swallows the click.
 * Both specs override the wrapper back to `none` via the node's own `style`
 * (xyflow spreads `node.style` AFTER its own `pointerEvents`, so this wins
 * without `!important`), and the individual chips opt back in.
 *
 * WHY TWO NODES (#455). The box must paint BEHIND the member cards; the header
 * and the exhausted badge — which carries the #152 "route from manager" button —
 * must sit ABOVE them. Those are opposed, and one node cannot do both: a
 * positioned wrapper with a numeric `z-index` IS a stacking context, so no
 * descendant of the `zIndex: 0` box can ever paint above a sibling card. With
 * the chrome inside the box, any node overlapping the header band stole its
 * clicks and the RegionInspector became unreachable. So: a `box` layer at
 * `zIndex: 0`, and a `chrome` layer at {@link REGION_CHROME_Z} covering the same
 * rectangle. `LoopRegionNode` renders one or the other on `data.layer`.
 *
 * Both keep `type: "loopRegion"` on purpose — every `n.type === "loopRegion"`
 * guard in `EditCanvas` (drag-stop, context menu, node click) then covers the
 * chrome for free instead of needing a parallel set of guards for a 5th type.
 */
export function buildLoopRegionNodes(
  pipeline: EditorPipelineDef,
  runState: RunState | null | undefined,
): Node[] {
  return deriveLoopRegions(pipeline, runState)
    .filter((r) => r.box != null)
    .flatMap((r) => {
      const position = { x: r.box!.x, y: r.box!.y };
      const data = {
        regionId: r.id,
        kind: r.kind,
        counterText: r.counterText,
        exhausted: r.exhausted,
        // The live run id (or null in a template view) so an exhausted-unrouted
        // region can offer "route from manager" (ADR-0011 / #152).
        runId: runState?.run_id ?? null,
        width: r.box!.width,
        height: r.box!.height,
      };
      const inert = {
        type: "loopRegion",
        position,
        draggable: false,
        selectable: false,
        connectable: false,
        focusable: false,
      } as const;
      return [
        {
          ...inert,
          id: `region-${r.id}`,
          data: { ...data, layer: "box" as const },
          zIndex: 0,
          style: { zIndex: 0, pointerEvents: "none" },
        },
        {
          ...inert,
          id: `region-chrome-${r.id}`,
          data: { ...data, layer: "chrome" as const },
          zIndex: REGION_CHROME_Z,
          style: { zIndex: REGION_CHROME_Z, pointerEvents: "none" },
        },
      ];
    });
}
