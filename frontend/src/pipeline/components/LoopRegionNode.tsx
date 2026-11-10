import { type Node, type NodeProps } from "@xyflow/react";
import { Repeat, Split, Wallet } from "lucide-react";
import { Button } from "../../shared/ui/kit";
import type { LoopKind } from "../../shared/generated/LoopKind";
import { useEditStore } from "../store/editStore";
import { useRaiseCap } from "../store/useRaiseCap";
import type { RunCapSource } from "../model/runCostCaps";
import { endRegion } from "../../run/api/commands";
import { useWhyNotOnRun } from "../../run/store/gestureRightsStore";
import { toastRefusal } from "../../shared/authorization/refusal";

/**
 * Data carried by a `loopRegion` canvas node (ADR-0011 / #148, #150). A loop
 * region is the named `loops:` entry, NOT a pipeline node: the canvas draws it
 * as a translucent box enclosing its members (>= 2 members) with a `↻ X/Y`
 * header. Single-member regions render as a badge instead (handled on the member
 * card), so this node only ever backs the box form. The box is purely
 * decorative — it sits behind the member cards and routes clicks through to the
 * canvas — but its header is clickable: it opens the region inspector, which is
 * the *sole* place `max_iter` is edited and where the region id is shown. The
 * canvas header carries neither an inline control nor the id, per the slim-card
 * rule (#149): the inline header editor #150 originally added was removed.
 *
 * Each region is backed by TWO such nodes, distinguished by {@link layer} —
 * see `buildLoopRegionNodes` for why one cannot do both jobs (#455).
 */
export interface LoopRegionNodeData {
  regionId: string;
  kind: LoopKind;
  /**
   * `↻` counter text, e.g. `max 3` (idle) or `2/3` (running). Read-only on the
   * canvas — the bound is edited in the RegionInspector, never inline here.
   */
  counterText: string;
  /** The Run is parked « exhausted — unrouted » on this region. */
  exhausted: boolean;
  /**
   * The corner's money (ADR draft plafond-de-cout-de-boucle): the Coût de
   * boucle on a Run (`~$1.20`), with its cap (`~$1.20 / $5`), or the cap alone
   * off a Run (`≤ $5`). Absent when there is nothing to say.
   */
  costText?: string | null;
  /** The cost's tooltip: what it covers and each member's share. */
  costTitle?: string | null;
  /** The cost reached 80 % of the cap: only its text turns amber. */
  costWarn?: boolean;
  /** The Run is parked « budget reached » on this region. */
  budgetReached?: boolean;
  /**
   * The live run this region belongs to, or `null` in a template view. Present
   * only when a run is active; the "route from manager" affordance (#152) on an
   * exhausted-unrouted region targets this run.
   */
  runId: string | null;
  /** What the Run says about its caps: `raise cap` starts editing from them. */
  capSource?: RunCapSource | null;
  width: number;
  height: number;
  /**
   * Which half of the region this node draws (#455).
   *
   * `"box"` — the dashed, translucent rectangle, pinned BEHIND the member cards.
   * `"chrome"` — the same rectangle, transparent, ABOVE the cards, carrying the
   * clickable header and the exhausted badge.
   *
   * They cannot be one node: a positioned wrapper with a numeric `z-index` is a
   * stacking context, so chrome nested in the `zIndex: 0` box could never paint
   * above a card, and a card overlapping the header band swallowed its clicks.
   */
  layer: "box" | "chrome";
  [key: string]: unknown;
}

export function LoopRegionNode({ data }: NodeProps<Node<LoopRegionNodeData>>) {
  const setSelection = useEditStore((s) => s.setSelection);
  const selected = useEditStore(
    (s) => s.selection.kind === "region" && s.selection.regionId === data.regionId,
  );
  // CORE-42, CORE-205: « route from manager » greyed, with why, when the caller may not end the region.
  const endRegionRefused = useWhyNotOnRun(data.runId as string | null | undefined)("end_region");
  const collection = data.kind === "collection";
  const budget = data.budgetReached === true;
  // A fan-out splits its members, a bounded loop repeats them; a loop that
  // reached its cost cap shows a wallet.
  const Glyph = budget ? Wallet : collection ? Split : Repeat;

  const openInspector = () =>
    setSelection({ kind: "region", id: null, regionId: data.regionId });
  const raiseCap = useRaiseCap();

  // The grouping layer: a tinted area, nothing interactive. Sits behind the
  // member cards and lets every click through (#167).
  if (data.layer === "box") {
    return (
      <div
        data-testid="loop-region"
        data-region-id={data.regionId}
        data-exhausted={data.exhausted || undefined}
        data-budget={budget || undefined}
        data-selected={selected || undefined}
        className="loop-region cv-region"
        style={{ width: data.width, height: data.height }}
      />
    );
  }

  // The chrome layer: same rectangle, transparent (it covers the cards),
  // `pointer-events: none` throughout except on the counter and the badge.
  const title = [
    `${collection ? "Collection region" : "Loop region"} ${data.regionId} · ${data.counterText}${
      data.exhausted ? " · limit reached" : budget ? " · budget reached" : ""
    }`,
    data.costTitle,
  ]
    .filter(Boolean)
    .join("\n");
  return (
    <div
      data-testid="loop-region-chrome"
      data-region-id={data.regionId}
      className="cv-region-chrome"
      style={{ width: data.width, height: data.height }}
    >
      {/* The counter, top right: read-only — `max_iter` is edited in the
          RegionInspector, where the region id is shown too (#149). */}
      <button
        type="button"
        data-testid="loop-region-header"
        data-exhausted={data.exhausted || undefined}
        data-budget={budget || undefined}
        onClick={openInspector}
        className="nodrag nopan cv-region__corner"
        title={title}
      >
        <Glyph size={11} strokeWidth={1.5} className="loop-region-glyph" aria-hidden />
        <span className="loop-region-count">{data.counterText}</span>
        {data.costText != null && (
          <>
            <span aria-hidden className="cv-region__sep">
              ·
            </span>
            <span
              data-testid="loop-region-cost"
              className="cv-region__cost"
              data-warn={data.costWarn || undefined}
            >
              {data.costText}
            </span>
          </>
        )}
      </button>
      {budget && (
        <div data-testid="loop-region-budget" className="cv-region__block" data-budget>
          budget reached
          {data.runId != null && (
            <>
              {/* Raising the cap is an edit of the Run's bounds: « Edit for
                  this run », the region inspector on `max_cost_usd`. Like the
                  corner, the click reaches the canvas's node click, where
                  Pipeline info gives way. */}
              <Button
                size="sm"
                data-testid="loop-region-raise-cap"
                onClick={() => raiseCap(data.runId as string, data.regionId, data.capSource)}
              >
                raise cap
              </Button>
              <Button
                size="sm"
                tone="ghost"
                data-testid="loop-region-end"
                disabledReason={endRegionRefused}
                onClick={(e) => {
                  e.stopPropagation();
                  endRegion(data.runId as string, data.regionId).catch(toastRefusal);
                }}
              >
                end region
              </Button>
            </>
          )}
        </div>
      )}
      {data.exhausted && (
        <div data-testid="loop-region-block" className="cv-region__block">
          exhausted · unrouted
          {data.runId != null && (
            // The run overlay on an exhausted-unrouted region offers a "route
            // from manager" affordance (ADR-0011 / #152): ending the region by
            // id fires its completion and the daemon resumes the run.
            <Button
              size="sm"
              data-testid="loop-region-route-from-manager"
              disabledReason={endRegionRefused}
              onClick={(e) => {
                e.stopPropagation();
                endRegion(data.runId as string, data.regionId).catch(toastRefusal);
              }}
            >
              Route from manager
            </Button>
          )}
        </div>
      )}
    </div>
  );
}
