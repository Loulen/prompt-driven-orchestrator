import { type Node, type NodeProps } from "@xyflow/react";
import type { LoopKind } from "../types";

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
  width: number;
  height: number;
  [key: string]: unknown;
}

export function LoopRegionNode({ data }: NodeProps<Node<LoopRegionNodeData>>) {
  const accent = data.exhausted
    ? "var(--color-st-blocked)"
    : "var(--color-acc)";
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
          ↻
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
        </div>
      )}
    </div>
  );
}
