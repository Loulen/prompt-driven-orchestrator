// The wiring grid (#844), drawn in FLOW coordinates.
//
// NOT xyflow's `<Background>`: its pattern lives in screen space, so the dots and
// the snapped path agree only at zoom 1 with the canvas unpanned — and never after
// a Shift re-origin. Rendered inside `<ViewportPortal>` the overlay shares the
// path's own coordinate system, so a dot IS a point the trace can snap to, at any
// zoom, with no arithmetic to keep in sync. The decorative 20px background grid is
// a separate, independent thing (CONTEXT.md § « Grille de câblage »).
//
// The grid is a CSS background on one big div rather than thousands of SVG
// elements: the browser tiles it, and `background-position` gives the lattice its
// origin for free.

import { useStore } from "@xyflow/react";
import type { Point } from "../lib/orthogonalRouter";
import { WIRE } from "../lib/wiringColors";
import { crispOffset } from "../lib/wiringGrid";

/** Half-extent of the overlay, in flow px. A multiple of every step so the
 *  lattice stays in phase with the origin at the div's own edges. */
const HALF = 4000;

/** Line width and dot radius, in SCREEN pixels — the width they have to keep,
 *  whatever the zoom, to survive rasterisation. */
const LINE_PX = 1;
const DOT_PX = 0.8;

/**
 * The same overlay, reading the viewport zoom and translate itself.
 *
 * Subscribed here rather than in `EditCanvas` so a pan or zoom re-renders the grid
 * and nothing else; the pure component below stays renderable without a flow
 * provider.
 */
export function ViewportWiringGridOverlay(props: { origin: Point; step: number; variant: "dots" | "lines" }) {
  const zoom = useStore((s) => s.transform[2]);
  const tx = useStore((s) => s.transform[0]);
  const ty = useStore((s) => s.transform[1]);
  return <WiringGridOverlay {...props} zoom={zoom} translate={{ x: tx, y: ty }} />;
}

export default function WiringGridOverlay({
  origin,
  step,
  variant,
  zoom = 1,
  translate = { x: 0, y: 0 },
}: {
  origin: Point;
  step: number;
  variant: "dots" | "lines";
  /** Viewport zoom. The overlay lives in FLOW space, so the browser rasterises it
   *  scaled by this: a width expressed in flow px is multiplied by it. */
  zoom?: number;
  /** Viewport translate, in screen px — only used to keep the lines crisp. */
  translate?: Point;
}) {
  // Every stroke is drawn a flow-px width that COMES OUT at its screen width.
  // Without this the `lines` variant was invisible at the zoom users actually sit
  // at: a 0.5px flow line at the default fit zoom is under half a device pixel and
  // Chrome rasterises it away entirely — the setting was shipped painting nothing,
  // and only showed up past zoom 2 where half a flow pixel finally reaches one
  // device pixel (#844, FP finding 1).
  const px = 1 / Math.max(zoom, 0.05);
  // A dot from `radial-gradient` sits at the CENTRE of its tile, so the lattice is
  // shifted back half a cell to put the dots on the grid lines themselves — the
  // points the trace actually snaps to.
  const half = step / 2;
  const line = LINE_PX * px;
  const dot = DOT_PX * px;
  const left = origin.x - HALF;
  const top = origin.y - HALF;
  const nudgeX = crispOffset(left, translate.x, zoom);
  const nudgeY = crispOffset(top, translate.y, zoom);
  const style: React.CSSProperties =
    variant === "dots"
      ? {
          backgroundImage: `radial-gradient(circle, ${WIRE} ${dot}px, transparent ${dot + 0.1 * px}px)`,
          backgroundSize: `${step}px ${step}px`,
          backgroundPosition: `${-half}px ${-half}px`,
          opacity: 0.55,
        }
      : {
          backgroundImage:
            `linear-gradient(to right, ${WIRE} 0 ${line}px, transparent ${line}px),` +
            `linear-gradient(to bottom, ${WIRE} 0 ${line}px, transparent ${line}px)`,
          backgroundSize: `${step}px ${step}px, ${step}px ${step}px`,
          backgroundPosition: `${nudgeX}px 0, 0 ${nudgeY}px`,
          opacity: 0.35,
        };

  return (
    <div
      data-testid="wiring-grid"
      style={{
        position: "absolute",
        // Anchored ON the origin, so the lattice passes exactly through it.
        left,
        top,
        width: HALF * 2,
        height: HALF * 2,
        pointerEvents: "none",
        // UNDER the edges and the cards, as in the validated prototype. The
        // viewport portal comes last in the viewport, so without this the grid
        // was painted across every card during a drag (#844 FP iter-2). The
        // viewport is its own stacking context, so -1 stays above the canvas
        // background.
        zIndex: -1,
        ...style,
      }}
    />
  );
}
