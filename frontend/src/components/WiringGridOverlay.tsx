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

import type { Point } from "../lib/orthogonalRouter";
import { WIRE } from "../lib/wiringColors";

/** Half-extent of the overlay, in flow px. A multiple of every step so the
 *  lattice stays in phase with the origin at the div's own edges. */
const HALF = 4000;

export default function WiringGridOverlay({
  origin,
  step,
  variant,
}: {
  origin: Point;
  step: number;
  variant: "dots" | "lines";
}) {
  // A dot from `radial-gradient` sits at the CENTRE of its tile, so the lattice is
  // shifted back half a cell to put the dots on the grid lines themselves — the
  // points the trace actually snaps to.
  const half = step / 2;
  const style: React.CSSProperties =
    variant === "dots"
      ? {
          backgroundImage: `radial-gradient(circle, ${WIRE} 0.8px, transparent 0.9px)`,
          backgroundSize: `${step}px ${step}px`,
          backgroundPosition: `${-half}px ${-half}px`,
          opacity: 0.55,
        }
      : {
          backgroundImage:
            `linear-gradient(to right, ${WIRE} 0 0.5px, transparent 0.5px),` +
            `linear-gradient(to bottom, ${WIRE} 0 0.5px, transparent 0.5px)`,
          backgroundSize: `${step}px ${step}px, ${step}px ${step}px`,
          backgroundPosition: "0 0, 0 0",
          opacity: 0.35,
        };

  return (
    <div
      data-testid="wiring-grid"
      style={{
        position: "absolute",
        // Anchored ON the origin, so the lattice passes exactly through it.
        left: origin.x - HALF,
        top: origin.y - HALF,
        width: HALF * 2,
        height: HALF * 2,
        pointerEvents: "none",
        ...style,
      }}
    />
  );
}
