import { type Node, type NodeProps } from "@xyflow/react";
import type { LoopKind } from "../types";
import { endRegion } from "../api";

/**
 * Data carried by a `loopRegion` canvas node (ADR-0011 / #148). A loop region is
 * the named `loops:` entry, NOT a pipeline node: the canvas draws it as a
 * translucent box enclosing its members (>= 2 members) with a `↻ X/Y` header.
 * Single-member regions render as a badge instead (handled on the member card),
 * so this node only ever backs the box form. The node is purely decorative — it
 * sits behind the member cards, is not draggable/connectable, and routes clicks
 * through to the canvas except on its header.
 */
export interface LoopRegionNodeData {
  regionId: string;
  kind: LoopKind;
  /** `↻` counter text, e.g. `max 3` (idle) or `2/3` (running). */
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
  [key: string]: unknown;
}

export function LoopRegionNode({ data }: NodeProps<Node<LoopRegionNodeData>>) {
  const accent = data.exhausted
    ? "var(--color-st-blocked)"
    : "var(--color-acc)";
  // `⇉` (fan-out) for a collection region, `↻` (loop) for a bounded one.
  const glyph = data.kind === "collection" ? "⇉" : "↻";
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
    >
      <div
        data-testid="loop-region-header"
        className="pointer-events-auto absolute flex items-center gap-1.5 rounded bg-bg-1 px-2 font-mono"
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
        <span className="loop-region-count" style={{ opacity: 0.85 }}>
          {data.counterText}
        </span>
        <span className="loop-region-name pl-0.5 font-normal text-fg-3">
          {data.regionId}
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
