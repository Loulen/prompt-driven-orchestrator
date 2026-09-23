import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import WiringGridOverlay from "./WiringGridOverlay";
import { WIRING_GRID_STEP, crispOffset, snapToGrid } from "../lib/wiringGrid";

describe("WiringGridOverlay (#844)", () => {
  it("anchors the lattice ON the origin, so a dot is a point the trace snaps to", () => {
    // A re-origined grid (Shift released at an off-lattice point) still has to
    // agree with the path: the overlay lives in FLOW coordinates, so its own
    // left/top follow the origin rather than the screen.
    const origin = { x: 57, y: 123 };
    render(<WiringGridOverlay origin={origin} step={WIRING_GRID_STEP} variant="dots" />);
    const grid = screen.getByTestId("wiring-grid");
    const left = parseFloat(grid.style.left);
    const top = parseFloat(grid.style.top);
    // The overlay's own edges are a whole number of cells from the origin, so the
    // tiled lattice stays in phase with it.
    expect((origin.x - left) % WIRING_GRID_STEP).toBe(0);
    expect((origin.y - top) % WIRING_GRID_STEP).toBe(0);
    // And the lattice it draws is the one `snapToGrid` snaps onto.
    expect(snapToGrid(origin, origin, WIRING_GRID_STEP)).toEqual(origin);
  });

  it("puts the dots on the grid lines, not at the tile centres", () => {
    render(<WiringGridOverlay origin={{ x: 0, y: 0 }} step={WIRING_GRID_STEP} variant="dots" />);
    const grid = screen.getByTestId("wiring-grid");
    // A `radial-gradient` dot sits at the CENTRE of its tile; shifting the
    // background back half a cell is what moves it onto the line.
    expect(grid.style.backgroundPosition).toBe(
      `${-WIRING_GRID_STEP / 2}px ${-WIRING_GRID_STEP / 2}px`,
    );
    expect(grid.style.backgroundSize).toBe(`${WIRING_GRID_STEP}px ${WIRING_GRID_STEP}px`);
  });

  it("draws two crossing gradients in the lines variant", () => {
    render(<WiringGridOverlay origin={{ x: 0, y: 0 }} step={WIRING_GRID_STEP} variant="lines" />);
    const grid = screen.getByTestId("wiring-grid");
    expect(grid.style.backgroundImage).toContain("to right");
    expect(grid.style.backgroundImage).toContain("to bottom");
  });

  // The overlay is drawn in FLOW coordinates, so the browser rasterises it scaled
  // by the zoom: a width written in flow px comes out multiplied by it. Half a
  // flow pixel — what the `lines` variant used to ask for — is under half a device
  // pixel at the zoom a canvas fits itself to, and Chrome rasterises it away
  // completely. The setting was shipped painting nothing (#844, FP finding 1).
  const widths = (variant: "dots" | "lines", zoom: number): number[] => {
    render(
      <WiringGridOverlay
        origin={{ x: 0, y: 0 }}
        step={WIRING_GRID_STEP}
        variant={variant}
        zoom={zoom}
      />,
    );
    const grid = screen.getAllByTestId("wiring-grid").pop()!;
    return [...grid.style.backgroundImage.matchAll(/([\d.]+)px/g)].map((m) => Number(m[1]));
  };

  it.each([0.5, 0.9614, 1, 2])("keeps every wiring-grid line a screen pixel wide at zoom %s", (zoom) => {
    for (const w of widths("lines", zoom)) {
      expect(w * zoom).toBeCloseTo(1, 5);
    }
  });

  it("scales the dots the same way, so they survive a zoomed-out canvas too", () => {
    const [radius] = widths("dots", 0.5);
    expect(radius * 0.5).toBeCloseTo(0.8, 5);
  });

  it("nudges the lines onto whole screen pixels under a fractional translate (zoom 1)", () => {
    // xyflow's fit view leaves the viewport at x.5: a 1px line starting there is
    // two half-intensity columns.
    render(
      <WiringGridOverlay
        origin={{ x: 0, y: 0 }}
        step={WIRING_GRID_STEP}
        variant="lines"
        zoom={1}
        translate={{ x: 120.5, y: 33.25 }}
      />,
    );
    const grid = screen.getByTestId("wiring-grid");
    const [vx, , , vy] = grid.style.backgroundPosition
      .split(/[ ,]+/)
      .map((v) => parseFloat(v));
    const left = parseFloat(grid.style.left);
    const top = parseFloat(grid.style.top);
    // Every vertical line starts on a whole screen pixel…
    for (const k of [0, 1, 7]) {
      const screenX = 120.5 + (left + vx + k * WIRING_GRID_STEP);
      expect(Math.abs(screenX - Math.round(screenX))).toBeLessThan(1e-9);
      const screenY = 33.25 + (top + vy + k * WIRING_GRID_STEP);
      expect(Math.abs(screenY - Math.round(screenY))).toBeLessThan(1e-9);
    }
    // …by moving it less than half a screen pixel off the lattice.
    expect(Math.abs(vx)).toBeLessThanOrEqual(0.5);
    expect(Math.abs(vy)).toBeLessThanOrEqual(0.5);
  });

  it("does not nudge at all when the translate is already whole", () => {
    expect(crispOffset(-4000, 120, 1)).toBe(0);
    expect(crispOffset(-4000, 120, 0.5)).toBe(0);
  });

  it("paints under the edges and the cards", () => {
    render(<WiringGridOverlay origin={{ x: 0, y: 0 }} step={WIRING_GRID_STEP} variant="lines" />);
    expect(screen.getByTestId("wiring-grid").style.zIndex).toBe("-1");
  });

  it("never eats a pointer event — it is a reading aid, not a surface", () => {
    render(<WiringGridOverlay origin={{ x: 0, y: 0 }} step={WIRING_GRID_STEP} variant="dots" />);
    expect(screen.getByTestId("wiring-grid").style.pointerEvents).toBe("none");
  });
});
