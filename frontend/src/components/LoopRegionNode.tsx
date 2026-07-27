import { type Node, type NodeProps } from "@xyflow/react";
import type { LoopKind } from "../types";
import { useEditStore } from "../stores/editStore";
import { endRegion } from "../api";

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
  /** True once the region reached `max_iter` with the loop still continuing. */
  exhausted: boolean;
  /**
   * The live run this region belongs to, or `null` in a template view. Present
   * only when a run is active; the "route from manager" affordance (#152) on an
   * exhausted-unrouted region targets this run.
   */
  runId: string | null;
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

  const accent = data.exhausted
    ? "var(--color-st-blocked)"
    : "var(--color-acc)";
  // `⇉` (fan-out) for a collection region, `↻` (loop) for a bounded one.
  const glyph = data.kind === "collection" ? "⇉" : "↻";

  const openInspector = () =>
    setSelection({ kind: "region", id: null, regionId: data.regionId });

  // The grouping layer: border + faint fill, nothing interactive. Sits behind
  // the member cards and lets every click through (#167).
  if (data.layer === "box") {
    return (
      <div
        data-testid="loop-region"
        data-region-id={data.regionId}
        className="loop-region pointer-events-none relative"
        style={{
          width: data.width,
          height: data.height,
          borderRadius: 12,
          border: `1px dashed ${accent}`,
          // Faint translucent fill so the box reads as a grouping layer behind
          // the member cards without obscuring them.
          background: data.exhausted
            ? "var(--color-st-blocked-bg)"
            : "var(--color-acc-bg)",
        }}
      />
    );
  }

  // The chrome layer: same rectangle, no border and no fill (it covers the
  // cards, so anything painted here would hide them), `pointer-events: none`
  // throughout except on the two chips below.
  return (
    <div
      data-testid="loop-region-chrome"
      data-region-id={data.regionId}
      className="pointer-events-none relative"
      style={{ width: data.width, height: data.height }}
    >
      <div
        data-testid="loop-region-header"
        onClick={openInspector}
        className="pointer-events-auto absolute flex cursor-pointer items-center gap-1.5 rounded bg-bg-1 px-2 font-mono"
        style={{
          top: -13,
          left: 14,
          height: 23,
          fontSize: 11,
          fontWeight: 500,
          border: `1px solid ${accent}`,
          color: accent,
        }}
      >
        <span className="loop-region-glyph" style={{ fontSize: 12, lineHeight: 1 }}>
          {glyph}
        </span>
        {/* Read-only counter. `max_iter` is edited in the RegionInspector, and
            the region id is shown there too — the canvas header carries neither
            an inline control nor the id, honouring the slim-card rule (#149). */}
        <span className="loop-region-count" style={{ opacity: 0.85 }}>
          {data.counterText}
        </span>
      </div>
      {data.exhausted && (
        <div
          data-testid="loop-region-block"
          className="pointer-events-auto absolute flex items-center gap-1.5 rounded bg-bg-1 px-2 font-mono whitespace-nowrap"
          style={{
            bottom: -14,
            left: "50%",
            transform: "translateX(-50%)",
            height: 23,
            fontSize: 10,
            border: "1px solid var(--color-st-blocked)",
            color: "var(--color-st-blocked)",
          }}
        >
          exhausted — unrouted
          {data.runId != null && (
            // The run overlay on an exhausted-unrouted region offers a "route
            // from manager" affordance (ADR-0011 / #152): ending the region by
            // id fires its completion and the daemon resumes the run.
            <button
              type="button"
              data-testid="loop-region-route-from-manager"
              className="ml-1.5 rounded border px-1 leading-none hover:bg-st-blocked-bg"
              style={{
                borderColor: "var(--color-st-blocked)",
                color: "var(--color-st-blocked)",
              }}
              onClick={(e) => {
                e.stopPropagation();
                void endRegion(data.runId as string, data.regionId);
              }}
            >
              route from manager
            </button>
          )}
        </div>
      )}
    </div>
  );
}
