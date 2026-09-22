import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import WiringGridOverlay from "./WiringGridOverlay";
import { WIRING_GRID_STEP, snapToGrid } from "../lib/wiringGrid";

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

  it("never eats a pointer event — it is a reading aid, not a surface", () => {
    render(<WiringGridOverlay origin={{ x: 0, y: 0 }} step={WIRING_GRID_STEP} variant="dots" />);
    expect(screen.getByTestId("wiring-grid").style.pointerEvents).toBe("none");
  });
});
