import { describe, it, expect } from "vitest";
import type { Point } from "./orthogonalRouter";
import {
  advanceTrace,
  dragSegmentOnGrid,
  elbow,
  freeContinuation,
  isAxisAligned,
  mergeAlignedSegments,
  mergeCollinear,
  reOrigin,
  segmentAxis,
  snapPolyline,
  snapToGrid,
  snapStepAtZoom,
  snapBadgeText,
  startTrace,
  DEFAULT_GRID_SIZE,
  GRID_SIZES,
  gridStep,
  isGridSize,
  resolveGridSize,
} from "./wiringGrid";

const ORIGIN: Point = { x: 0, y: 0 };
// The geometry fixtures below are laid out on the 40px (L) lattice.
const STEP = gridStep("L");

/** Every consecutive pair shares an axis. */
function orthogonal(points: Point[]): boolean {
  for (let i = 1; i < points.length; i++) {
    if (!isAxisAligned(points[i - 1], points[i])) return false;
  }
  return true;
}

describe("the wiring grid sizes (#877 / ADR-0076)", () => {
  it("offers S, M and L at 20, 30 and 40px", () => {
    expect(GRID_SIZES).toEqual(["S", "M", "L"]);
    expect(GRID_SIZES.map(gridStep)).toEqual([20, 30, 40]);
  });

  it("defaults to M — a pipeline nobody configured draws on 30px", () => {
    expect(DEFAULT_GRID_SIZE).toBe("M");
    expect(gridStep(resolveGridSize(undefined, DEFAULT_GRID_SIZE))).toBe(30);
  });

  it("lets the pipeline's own size win over the global default", () => {
    expect(resolveGridSize("S", "L")).toBe("S");
    expect(resolveGridSize(null, "L")).toBe("L");
    expect(resolveGridSize(undefined, "S")).toBe("S");
  });

  it("reads an unknown size (hand-edited file) as no choice", () => {
    expect(isGridSize("XL")).toBe(false);
    expect(resolveGridSize("XL", "M")).toBe("M");
  });

  it("snaps a trace on the resolved step, for every size", () => {
    for (const size of GRID_SIZES) {
      const step = gridStep(size);
      const p = snapToGrid({ x: 97, y: 61 }, ORIGIN, step);
      expect(p.x % step).toBe(0);
      expect(p.y % step).toBe(0);
    }
    expect(snapToGrid({ x: 97, y: 61 }, ORIGIN, gridStep("M"))).toEqual({ x: 90, y: 60 });
  });
});

describe("snapToGrid", () => {
  it("snaps onto the lattice through the origin", () => {
    expect(snapToGrid({ x: 97, y: -13 }, ORIGIN, STEP)).toEqual({ x: 80, y: 0 });
    expect(snapToGrid({ x: 101, y: 61 }, ORIGIN, STEP)).toEqual({ x: 120, y: 80 });
  });

  it("carries an off-lattice origin, so a re-origined grid passes through it", () => {
    const origin = { x: 13, y: 7 };
    expect(snapToGrid({ x: 14, y: 8 }, origin, STEP)).toEqual(origin);
    expect(snapToGrid({ x: 60, y: 50 }, origin, STEP)).toEqual({ x: 53, y: 47 });
  });

  it("leaves a point already on the lattice where it is", () => {
    const p = { x: 200, y: -120 };
    expect(snapToGrid(p, ORIGIN, STEP)).toEqual(p);
  });
});

describe("segmentAxis / mergeCollinear", () => {
  it("names the axis a segment travels on, and nothing for a zero-length one", () => {
    expect(segmentAxis({ x: 0, y: 0 }, { x: 40, y: 0 })).toBe("horizontal");
    expect(segmentAxis({ x: 0, y: 0 }, { x: 0, y: 40 })).toBe("vertical");
    expect(segmentAxis({ x: 3, y: 3 }, { x: 3, y: 3 })).toBeNull();
  });

  it("drops the middle of a straight run and exact duplicates", () => {
    expect(
      mergeCollinear([
        { x: 0, y: 0 },
        { x: 40, y: 0 },
        { x: 40, y: 0 },
        { x: 80, y: 0 },
        { x: 80, y: 40 },
      ]),
    ).toEqual([
      { x: 0, y: 0 },
      { x: 80, y: 0 },
      { x: 80, y: 40 },
    ]);
  });

  it("keeps a genuine bend", () => {
    const bent = [
      { x: 0, y: 0 },
      { x: 40, y: 0 },
      { x: 40, y: 40 },
    ];
    expect(mergeCollinear(bent)).toEqual(bent);
  });
});

describe("elbow", () => {
  it("leaves along the incoming axis first, then turns", () => {
    expect(elbow({ x: 0, y: 0 }, { x: 80, y: 40 }, "horizontal")).toEqual({ x: 80, y: 0 });
    expect(elbow({ x: 0, y: 0 }, { x: 80, y: 40 }, "vertical")).toEqual({ x: 0, y: 40 });
  });
});

describe("advanceTrace — one segment per crossed cell", () => {
  it("slides the tip along the current axis without adding a bend", () => {
    let t = startTrace({ x: 0, y: 0 }, "vertical");
    t = advanceTrace(t, { x: 0, y: 40 });
    t = advanceTrace(t, { x: 0, y: 80 });
    expect(t.points).toEqual([
      { x: 0, y: 0 },
      { x: 0, y: 80 },
    ]);
    expect(t.heading).toBe("vertical");
  });

  it("commits one elbow when the move leaves the current axis", () => {
    let t = startTrace({ x: 0, y: 0 }, "vertical");
    t = advanceTrace(t, { x: 0, y: 80 });
    t = advanceTrace(t, { x: 40, y: 80 });
    expect(t.points).toEqual([
      { x: 0, y: 0 },
      { x: 0, y: 80 },
      { x: 40, y: 80 },
    ]);
    expect(t.heading).toBe("horizontal");
    expect(orthogonal(t.points)).toBe(true);
  });

  it("un-draws the bend when the gesture walks back", () => {
    let t = startTrace({ x: 0, y: 0 }, "vertical");
    t = advanceTrace(t, { x: 0, y: 80 });
    t = advanceTrace(t, { x: 40, y: 80 });
    t = advanceTrace(t, { x: 0, y: 80 });
    expect(t.points).toEqual([
      { x: 0, y: 0 },
      { x: 0, y: 80 },
    ]);
  });

  it("stays on the lattice for a whole diagonal gesture", () => {
    let t = startTrace({ x: 0, y: 0 }, "vertical");
    for (let i = 1; i <= 6; i++) {
      t = advanceTrace(t, snapToGrid({ x: i * 37, y: i * 41 }, ORIGIN, STEP));
    }
    expect(orthogonal(t.points)).toBe(true);
    for (const p of t.points) {
      expect(p.x % STEP).toBe(0);
      expect(p.y % STEP).toBe(0);
    }
  });

  it("is a no-op when the cursor has not left the current cell", () => {
    const t = startTrace({ x: 0, y: 0 }, "vertical");
    expect(advanceTrace(t, { x: 0, y: 0 })).toBe(t);
  });
});

describe("Shift — free continuation and re-origin", () => {
  it("keeps the freed run an orthogonal L, never a staircase", () => {
    const t = startTrace({ x: 0, y: 0 }, "vertical");
    const freed = freeContinuation({ ...t, points: [{ x: 0, y: 0 }, { x: 0, y: 80 }] }, { x: 57, y: 123 });
    expect(freed).toEqual([
      { x: 0, y: 0 },
      { x: 0, y: 123 },
      { x: 57, y: 123 },
    ]);
    expect(orthogonal(freed)).toBe(true);
  });

  it("re-origins the lattice on the tip, so what follows lines up with what was drawn", () => {
    const tip = { x: 57, y: 123 };
    const origin = reOrigin(tip);
    expect(origin).toEqual(tip);
    // The next snapped point is on the NEW lattice: an exact number of cells
    // from the freehand tip, not from where the gesture started.
    const next = snapToGrid({ x: 150, y: 130 }, origin, STEP);
    expect((next.x - tip.x) % STEP).toBe(0);
    expect((next.y - tip.y) % STEP).toBe(0);
  });
});

describe("dragSegmentOnGrid", () => {
  // A three-segment staircase whose ends are pinned to their cards, deliberately
  // OFF the lattice — which is what a card-pinned endpoint looks like.
  const stair = (): Point[] => [
    { x: 7, y: 0 },
    { x: 7, y: 100 },
    { x: 200, y: 100 },
    { x: 200, y: 213 },
  ];

  it("moves only the dragged segment; the neighbours keep their exact coordinates", () => {
    const before = stair();
    const after = dragSegmentOnGrid(before, 1, 137, {
      origin: ORIGIN,
      step: STEP,
      free: false,
    });
    expect(after[0]).toEqual(before[0]);
    expect(after[3]).toEqual(before[3]);
    // The horizontal segment moved in y only; both its ends kept their x.
    expect(after[1].x).toBe(before[1].x);
    expect(after[2].x).toBe(before[2].x);
    expect(after[1].y).toBe(after[2].y);
    expect(orthogonal(after)).toBe(true);
  });

  it("snaps the dragged segment onto the lattice", () => {
    const after = dragSegmentOnGrid(stair(), 1, 137, { origin: ORIGIN, step: STEP, free: false });
    expect(after[1].y).toBe(120);
  });

  it("Shift frees it from the lattice", () => {
    const after = dragSegmentOnGrid(stair(), 1, 137, { origin: ORIGIN, step: STEP, free: true });
    expect(after[1].y).toBe(137);
  });

  it("magnets onto a collinear neighbour inside half a cell, so the merge is reachable", () => {
    // Snapping alone would put the segment on 200 or 240 and never on 213 — the
    // neighbour's own coordinate, which a card-pinned endpoint owns.
    const after = dragSegmentOnGrid(stair(), 1, 208, { origin: ORIGIN, step: STEP, free: false });
    expect(after[1].y).toBe(213);
    expect(after[2].y).toBe(213);
  });

  it("prefers the grid when no neighbour is within half a cell", () => {
    const after = dragSegmentOnGrid(stair(), 1, 50, { origin: ORIGIN, step: STEP, free: false });
    expect(after[1].y).toBe(40);
  });

  it("inserts a bend beside a pinned endpoint rather than moving it", () => {
    const straight: Point[] = [
      { x: 0, y: 0 },
      { x: 200, y: 0 },
    ];
    const after = dragSegmentOnGrid(straight, 0, 77, { origin: ORIGIN, step: STEP, free: false });
    expect(after[0]).toEqual({ x: 0, y: 0 });
    expect(after[after.length - 1]).toEqual({ x: 200, y: 0 });
    expect(after).toHaveLength(4);
    expect(orthogonal(after)).toBe(true);
  });

  it("is a no-op for an out-of-range segment index", () => {
    const before = stair();
    expect(dragSegmentOnGrid(before, 9, 10, { origin: ORIGIN, step: STEP, free: false })).toBe(before);
  });
});

describe("mergeAlignedSegments", () => {
  it("drops the waypoint between two segments a drag has aligned", () => {
    const aligned: Point[] = [
      { x: 7, y: 0 },
      { x: 7, y: 213 },
      { x: 200, y: 213 },
      { x: 200, y: 213 },
    ];
    expect(mergeAlignedSegments(aligned)).toEqual([
      { x: 7, y: 0 },
      { x: 7, y: 213 },
      { x: 200, y: 213 },
    ]);
  });

  it("leaves a genuine staircase alone", () => {
    const stair: Point[] = [
      { x: 0, y: 0 },
      { x: 0, y: 40 },
      { x: 40, y: 40 },
      { x: 40, y: 80 },
    ];
    expect(mergeAlignedSegments(stair)).toEqual(stair);
  });
});

describe("snapPolyline — the auto route, aligned on the grid", () => {
  it("puts the interior bends on the lattice while keeping every segment square", () => {
    const auto: Point[] = [
      { x: 3, y: 5 },
      { x: 97, y: 5 },
      { x: 97, y: 211 },
      { x: 300, y: 211 },
    ];
    const snapped = snapPolyline(auto, ORIGIN, STEP);
    expect(orthogonal(snapped)).toBe(true);
    // Endpoints belong to their nodes and never move.
    expect(snapped[0]).toEqual(auto[0]);
    expect(snapped[snapped.length - 1]).toEqual(auto[auto.length - 1]);
    // The free coordinate of each interior bend is on the lattice.
    expect(snapped[1].x % STEP).toBe(0);
  });

  it("leaves a two-point route alone — there is no interior to align", () => {
    const straight: Point[] = [
      { x: 1, y: 2 },
      { x: 3, y: 2 },
    ];
    expect(snapPolyline(straight, ORIGIN, STEP)).toBe(straight);
  });
});

describe("snapStepAtZoom — the gesture's lattice at any zoom (CORE-88, option A)", () => {
  it("keeps the pipeline step while a cell is at least 16px on screen", () => {
    expect(snapStepAtZoom(30, 1)).toEqual({ step: 30, factor: 1 });
    expect(snapStepAtZoom(20, 0.8)).toEqual({ step: 20, factor: 1 });
  });

  it("never subdivides the step when zoomed in: no half-step", () => {
    expect(snapStepAtZoom(30, 2)).toEqual({ step: 30, factor: 1 });
    expect(snapStepAtZoom(20, 4)).toEqual({ step: 20, factor: 1 });
  });

  it("doubles the step until a cell reaches 16px on screen when zoomed out", () => {
    // 30 × 0.25 = 7.5px → ×2 = 15px, still short → ×4 = 30px.
    expect(snapStepAtZoom(30, 0.25)).toEqual({ step: 120, factor: 4 });
    expect(snapStepAtZoom(30, 0.5)).toEqual({ step: 60, factor: 2 });
    expect(snapStepAtZoom(20, 0.4)).toEqual({ step: 40, factor: 2 });
  });

  it("only ever picks multiples, so every snapped point stays on the pipeline lattice", () => {
    const origin = { x: 7, y: -3 };
    for (const zoom of [0.1, 0.25, 0.33, 0.5, 0.75, 1, 1.5, 2]) {
      const { step } = snapStepAtZoom(30, zoom);
      expect(step % 30).toBe(0);
      const p = snapToGrid({ x: 123.4, y: 456.7 }, origin, step);
      expect(Math.abs((p.x - origin.x) % 30)).toBe(0);
      expect(Math.abs((p.y - origin.y) % 30)).toBe(0);
    }
  });

  it("names the lattice in the cursor badge", () => {
    expect(snapBadgeText(4, 120)).toBe("snap ×4 · 120px");
    expect(snapBadgeText(1, 30)).toBe("snap ×1");
  });

  it("falls back to the pipeline step for a degenerate zoom", () => {
    expect(snapStepAtZoom(30, 0)).toEqual({ step: 30, factor: 1 });
    expect(snapStepAtZoom(30, Number.NaN)).toEqual({ step: 30, factor: 1 });
  });
});
