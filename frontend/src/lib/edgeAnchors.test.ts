import { describe, it, expect } from "vitest";
import type { Point } from "./orthogonalRouter";
import {
  anchorFromPoint,
  anchorPoint,
  approachPoint,
  clipOutside,
  dragSegmentKeepingRun,
  enforcePerpendicularEnds,
  handlePin,
  landingConnector,
  landingLeg,
  dropAnchor,
  rimHandleId,
  sideFromRimHandle,
  sideIsHorizontal,
  squareUp,
  storableWaypoints,
} from "./anchorSide";
import { gridStep, isAxisAligned } from "./wiringGrid";

// Fixtures laid out on the 40px (L) lattice (#877: the step is a parameter).
const GRID_STEP = gridStep("L");

// A 200x80 card whose top-left is at (100, 100).
const RECT = { x: 100, y: 100, width: 200, height: 80 };
const LEG = landingLeg(GRID_STEP);

function orthogonal(points: Point[]): boolean {
  for (let i = 1; i < points.length; i++) {
    if (!isAxisAligned(points[i - 1], points[i])) return false;
  }
  return true;
}

describe("rim handles", () => {
  it("round-trips the side it encodes, and rejects anything else", () => {
    expect(sideFromRimHandle(rimHandleId("bottom"))).toBe("bottom");
    expect(sideFromRimHandle("__anchor:left")).toBeNull();
    expect(sideFromRimHandle("out")).toBeNull();
    expect(sideFromRimHandle(null)).toBeNull();
  });
});

describe("anchorPoint", () => {
  it("places the anchor along its side", () => {
    expect(anchorPoint(RECT, { side: "bottom", offset: 60 }, "bottom")).toEqual({ x: 160, y: 180 });
    expect(anchorPoint(RECT, { side: "left", offset: 30 }, "left")).toEqual({ x: 100, y: 130 });
    expect(anchorPoint(RECT, { side: "right", offset: 30 }, "right")).toEqual({ x: 300, y: 130 });
    expect(anchorPoint(RECT, { side: "top", offset: 60 }, "top")).toEqual({ x: 160, y: 100 });
  });

  it("falls back to the side's middle when there is no anchor — the pre-#844 geometry", () => {
    expect(anchorPoint(RECT, null, "bottom")).toEqual({ x: 200, y: 180 });
    expect(anchorPoint(RECT, undefined, "left")).toEqual({ x: 100, y: 140 });
  });

  it("keeps the anchor clear of the corners", () => {
    expect(anchorPoint(RECT, { side: "top", offset: -50 }, "top")).toEqual({ x: 110, y: 100 });
    expect(anchorPoint(RECT, { side: "top", offset: 9999 }, "top")).toEqual({ x: 290, y: 100 });
  });

  it("collapses to the middle on a card narrower than two margins", () => {
    const thin = { x: 0, y: 0, width: 8, height: 40 };
    expect(anchorPoint(thin, { side: "top", offset: 0 }, "top")).toEqual({ x: 4, y: 0 });
  });
});

describe("anchorFromPoint", () => {
  it("snaps the departure along the side onto the wiring lattice", () => {
    const a = anchorFromPoint({ x: 173, y: 400 }, RECT, "bottom", GRID_STEP);
    expect(a.side).toBe("bottom");
    // 173 snaps to 160 in absolute flow coordinates; the offset is relative.
    expect(a.offset).toBe(60);
    expect(anchorPoint(RECT, a, a.side).x % GRID_STEP).toBe(0);
  });

  it("takes the drop per pixel when the step is 0 — the arrow lands where it was aimed", () => {
    expect(anchorFromPoint({ x: 173, y: 400 }, RECT, "bottom", 0).offset).toBe(73);
  });

  it("measures a vertical side along y", () => {
    expect(sideIsHorizontal("left")).toBe(false);
    expect(anchorFromPoint({ x: 0, y: 137 }, RECT, "left", 0).offset).toBe(37);
  });
});

describe("dropAnchor", () => {
  it("lands on the side the drop was aimed at, at the position it was aimed at", () => {
    // RECT is 200x80 at (100,100); its centre is (200, 140).
    expect(dropAnchor({ x: 150, y: 105 }, RECT)).toEqual({ side: "top", offset: 50 });
    expect(dropAnchor({ x: 295, y: 150 }, RECT)).toEqual({ side: "right", offset: 50 });
    expect(dropAnchor({ x: 150, y: 175 }, RECT)).toEqual({ side: "bottom", offset: 50 });
    expect(dropAnchor({ x: 105, y: 150 }, RECT)).toEqual({ side: "left", offset: 50 });
  });

  it("picks the side by the centre ray, not by the nearest border (#219)", () => {
    // The default work card is short and wide. A drop in its left third is 30px
    // from the left border and 12px from the top one — « nearest border » would
    // anchor it TOP, which is the whole of #219. The centre ray says left.
    const wide = { x: 0, y: 0, width: 160, height: 35 };
    expect(dropAnchor({ x: 30, y: 12 }, wide).side).toBe("left");
    expect(dropAnchor({ x: 130, y: 23 }, wide).side).toBe("right");
    // Top/bottom still win when the drop really is above or below the diagonals.
    expect(dropAnchor({ x: 80, y: 2 }, wide).side).toBe("top");
    expect(dropAnchor({ x: 80, y: 33 }, wide).side).toBe("bottom");
  });
});

describe("landingConnector", () => {
  it("ends with the perpendicular leg, one cell long, pointing into the side", () => {
    const anchor = { x: 100, y: 140 };
    const pts = landingConnector({ x: 0, y: 0 }, anchor, "left", LEG);
    expect(orthogonal(pts)).toBe(true);
    const last = pts[pts.length - 1];
    const before = pts[pts.length - 2];
    expect(last).toEqual(anchor);
    // Horizontal, running rightwards INTO the left border, one leg long.
    expect(before.y).toBe(anchor.y);
    expect(anchor.x - before.x).toBe(LEG);
  });

  it("does the same on a top landing, with the leg vertical", () => {
    const anchor = { x: 160, y: 100 };
    const pts = landingConnector({ x: 0, y: 400 }, anchor, "top", LEG);
    expect(orthogonal(pts)).toBe(true);
    const before = pts[pts.length - 2];
    expect(before.x).toBe(anchor.x);
    expect(anchor.y - before.y).toBe(LEG);
  });
});

describe("landingConnector — around the card (#844, FP finding 2)", () => {
  /** Segments — the landing leg excepted — running through the card's inside. */
  function crossings(points: Point[], rect: typeof RECT): number {
    let n = 0;
    for (let i = 1; i < points.length - 1; i++) {
      const [a, b] = [points[i - 1], points[i]];
      const spans = (lo: number, hi: number, e0: number, e1: number) =>
        Math.min(lo, hi) < e1 && Math.max(lo, hi) > e0;
      if (
        spans(a.x, b.x, rect.x, rect.x + rect.width) &&
        spans(a.y, b.y, rect.y, rect.y + rect.height)
      ) {
        n++;
      }
    }
    return n;
  }

  it("does not walk through the card to reach a landing on its far side", () => {
    // Aiming at the BOTTOM border from above. The approach point is one leg BELOW
    // the card, so the plain L dives across the body, overshoots and doubles back
    // in — the spur the FP caught, persisted as waypoints inside the node.
    const anchor = { x: 200, y: 180 }; // middle of RECT's bottom border
    const from = { x: 120, y: 20 };
    expect(crossings(landingConnector(from, anchor, "bottom", LEG), RECT)).toBeGreaterThan(0);
    const around = landingConnector(from, anchor, "bottom", LEG, RECT);
    expect(orthogonal(around)).toBe(true);
    expect(crossings(around, RECT)).toBe(0);
    expect(around[around.length - 1]).toEqual(anchor);
  });

  it("turns the arrowhead the right way round when the wire is dead in line", () => {
    // Straight above the anchor, the plain connector collapsed to one segment
    // running from above INTO the bottom border: the wire pierced the card and
    // the arrowhead pointed out of it instead of in.
    const anchor = { x: 200, y: 180 };
    const straight = landingConnector({ x: 200, y: 20 }, anchor, "bottom", LEG);
    expect(straight[straight.length - 2].y).toBeLessThan(anchor.y);
    const around = landingConnector({ x: 200, y: 20 }, anchor, "bottom", LEG, RECT);
    expect(crossings(around, RECT)).toBe(0);
    // Now arriving from BELOW, one leg out, as a bottom landing must.
    expect(around[around.length - 2]).toEqual({ x: anchor.x, y: anchor.y + LEG });
  });

  it("leaves by the corridor on its own side of the card", () => {
    const anchor = { x: 200, y: 180 };
    // Above the card's left third: it comes down the LEFT, never across to the
    // far border. (A wire already clear of the card needs no detour at all — the
    // corridor only opens for one that would otherwise cross.)
    const left = landingConnector({ x: 120, y: 20 }, anchor, "bottom", LEG, RECT);
    expect(left[1].x).toBe(RECT.x - LEG);
    // …and the mirror image above its right third.
    const right = landingConnector({ x: 280, y: 20 }, anchor, "bottom", LEG, RECT);
    expect(right[1].x).toBe(RECT.x + RECT.width + LEG);
  });

  it("still lands perpendicular, one leg long, after the detour", () => {
    const anchor = { x: 100, y: 140 }; // middle of the LEFT border
    const pts = landingConnector({ x: 500, y: 140 }, anchor, "left", LEG, RECT);
    expect(crossings(pts, RECT)).toBe(0);
    const [before, last] = pts.slice(-2);
    expect(last).toEqual(anchor);
    expect(before.y).toBe(anchor.y);
    expect(anchor.x - before.x).toBe(LEG);
  });

  it("adds nothing when the landing is already reachable head-on", () => {
    const anchor = { x: 160, y: 100 };
    expect(landingConnector({ x: 0, y: 20 }, anchor, "top", LEG, RECT)).toEqual(
      landingConnector({ x: 0, y: 20 }, anchor, "top", LEG),
    );
  });
});

describe("handlePin", () => {
  it("pins on the middle of the handle's own side, from its centre", () => {
    // The End marker's `result` handle covers the whole card: its centre is the
    // card's centre, and the edge is pinned on the middle of its declared side —
    // the very point xyflow hands the renderer as `targetX/targetY`.
    const centre = { x: 200, y: 140 };
    const size = { width: 200, height: 80 };
    expect(handlePin(centre, size, "top")).toEqual({ x: 200, y: 100 });
    expect(handlePin(centre, size, "bottom")).toEqual({ x: 200, y: 180 });
    expect(handlePin(centre, size, "left")).toEqual({ x: 100, y: 140 });
    expect(handlePin(centre, size, "right")).toEqual({ x: 300, y: 140 });
  });

  it("is the side middle of the card for a card-sized handle", () => {
    const centre = { x: RECT.x + RECT.width / 2, y: RECT.y + RECT.height / 2 };
    for (const side of ["top", "bottom", "left", "right"] as const) {
      expect(handlePin(centre, RECT, side)).toEqual(anchorPoint(RECT, null, side));
    }
  });
});

describe("squareUp", () => {
  it("repairs a diagonal by INSERTING a bend, never by moving a point", () => {
    const pts = squareUp([
      { x: 0, y: 0 },
      { x: 100, y: 50 },
    ]);
    expect(orthogonal(pts)).toBe(true);
    expect(pts[0]).toEqual({ x: 0, y: 0 });
    expect(pts[pts.length - 1]).toEqual({ x: 100, y: 50 });
  });

  it("turns first rather than walking back over the segment it just drew", () => {
    // The wire leaves rightwards, then the next point is BEHIND: continuing
    // horizontally would retrace the leg.
    const pts = squareUp([
      { x: 0, y: 0 },
      { x: 40, y: 0 },
      { x: -60, y: 90 },
    ]);
    expect(orthogonal(pts)).toBe(true);
    // The inserted bend went vertical first (same x as the leg's end), so the
    // leg is not walked back over.
    expect(pts[2]).toEqual({ x: 40, y: 90 });
  });
});

describe("enforcePerpendicularEnds", () => {
  const sourceAnchor = { x: 160, y: 180 }; // bottom of RECT
  const targetAnchor = { x: 500, y: 300 }; // left side of some far card

  it("leaves and arrives perpendicular, with legs at least one cell long", () => {
    const pts = enforcePerpendicularEnds(
      [sourceAnchor, { x: 160, y: 400 }, { x: 320, y: 400 }, targetAnchor],
      "bottom",
      "left",
      LEG,
    );
    expect(orthogonal(pts)).toBe(true);
    expect(pts[0]).toEqual(sourceAnchor);
    expect(pts[pts.length - 1]).toEqual(targetAnchor);
    // First leg: vertical, downwards out of the bottom border.
    expect(pts[1].x).toBe(sourceAnchor.x);
    expect(pts[1].y - sourceAnchor.y).toBeGreaterThanOrEqual(LEG);
    // Last leg: horizontal, rightwards into the left border.
    const before = pts[pts.length - 2];
    expect(before.y).toBe(targetAnchor.y);
    expect(targetAnchor.x - before.x).toBeGreaterThanOrEqual(LEG);
  });

  it("keeps the arrow pointing INTO the card when the run arrives along the leg's axis", () => {
    // The collinear merge used to swallow the target approach here, leaving a
    // segment running outwards from inside the card.
    const pts = enforcePerpendicularEnds(
      [sourceAnchor, { x: 300, y: 300 }],
      "bottom",
      "left",
      LEG,
    );
    const before = pts[pts.length - 2];
    const last = pts[pts.length - 1];
    expect(last.x - before.x).toBeGreaterThan(0); // travelling right, into `left`
  });

  it("drops an interior point that landed exactly on an endpoint (no spur)", () => {
    const pts = enforcePerpendicularEnds(
      [sourceAnchor, { ...sourceAnchor }, { x: 160, y: 400 }, targetAnchor],
      "bottom",
      "left",
      LEG,
    );
    expect(orthogonal(pts)).toBe(true);
    // The wire never comes back to its own departure point after leaving it.
    expect(pts.slice(1).filter((p) => p.x === sourceAnchor.x && p.y === sourceAnchor.y)).toHaveLength(0);
  });

  it("is a no-op on a degenerate path", () => {
    expect(enforcePerpendicularEnds([sourceAnchor], "bottom", "left", LEG)).toEqual([sourceAnchor]);
  });

  it("is a fixed point: enforcing an enforced path changes nothing", () => {
    // What is rendered is what is saved. Without this, a bend inserted onto a
    // run it just completed survives into the file and the next render merges it
    // away — every open/save cycle would rewrite the route.
    const once = enforcePerpendicularEnds(
      [sourceAnchor, { x: 160, y: 400 }, { x: 320, y: 400 }, targetAnchor],
      "bottom",
      "left",
      LEG,
    );
    expect(enforcePerpendicularEnds(once, "bottom", "left", LEG)).toEqual(once);
  });
});

describe("enforcePerpendicularEnds — around the target card (#844 FP iter-3)", () => {
  // disk-janitor `start→reap`: Start's bottom output right ABOVE « Reclaim disk »,
  // whose input declares no side, so the arrow lands on the default left.
  const REAP = { x: 121, y: 40, width: 160, height: 36 };
  const src = { x: 200, y: -45 };
  const tgt = { x: 121, y: 58 };
  // What the router + lattice snap hand over: planned as if leaving rightwards.
  const auto = [src, { x: 160, y: -45 }, { x: 160, y: 58 }, tgt];

  function throughCard(points: Point[], rect: typeof REAP): boolean {
    for (let i = 1; i < points.length - 1; i++) {
      const [a, b] = [points[i - 1], points[i]];
      const spans = (lo: number, hi: number, e0: number, e1: number) =>
        Math.min(lo, hi) < e1 && Math.max(lo, hi) > e0;
      if (spans(a.x, b.x, rect.x, rect.x + rect.width) && spans(a.y, b.y, rect.y, rect.y + rect.height)) {
        return true;
      }
    }
    return false;
  }

  it("without the card, squares the route straight through it (the reported stub)", () => {
    const pts = enforcePerpendicularEnds(auto, "bottom", "left", LEG);
    expect(throughCard(pts, REAP)).toBe(true);
  });

  it("with the card, goes round its corner and runs into the left side head-on", () => {
    const pts = enforcePerpendicularEnds(auto, "bottom", "left", LEG, REAP);
    expect(pts).toEqual([
      src,
      { x: 200, y: -5 },
      { x: 81, y: -5 },
      { x: 81, y: 58 },
      tgt,
    ]);
    expect(throughCard(pts, REAP)).toBe(false);
    expect(orthogonal(pts)).toBe(true);
  });

  it("is still a fixed point, so the stored waypoints reload identically", () => {
    const once = enforcePerpendicularEnds(auto, "bottom", "left", LEG, REAP);
    expect(enforcePerpendicularEnds(once, "bottom", "left", LEG, REAP)).toEqual(once);
    const reloaded = enforcePerpendicularEnds(
      [src, ...storableWaypoints(once), tgt],
      "bottom",
      "left",
      LEG,
      REAP,
    );
    expect(reloaded).toEqual(once);
  });

  it("leaves a route that already clears the card untouched", () => {
    const clear = [src, { x: 200, y: 0 }, { x: 60, y: 0 }, { x: 60, y: 58 }, tgt];
    expect(enforcePerpendicularEnds(clear, "bottom", "left", LEG, REAP)).toEqual(
      enforcePerpendicularEnds(clear, "bottom", "left", LEG),
    );
  });
});

describe("enforcePerpendicularEnds — a manual route around its target (#844 FP iter-4)", () => {
  // « charlie »: a manual edge from Alpha's bottom into Charlie's (default) left
  // side, Charlie right below the source.
  const CHARLIE = { x: 121, y: 481, width: 160, height: 35 };
  const src = { x: 240, y: 195 };
  const tgt = { x: 121, y: 498 };
  const enforce = (interior: Point[]) =>
    enforcePerpendicularEnds([src, ...interior, tgt], "bottom", "left", LEG, CHARLIE);

  function throughCard(points: Point[]): boolean {
    for (let i = 1; i < points.length - 1; i++) {
      const [a, b] = [points[i - 1], points[i]];
      const spans = (lo: number, hi: number, e0: number, e1: number) =>
        Math.min(lo, hi) < e1 && Math.max(lo, hi) > e0;
      if (
        spans(a.x, b.x, CHARLIE.x, CHARLIE.x + CHARLIE.width) &&
        spans(a.y, b.y, CHARLIE.y, CHARLIE.y + CHARLIE.height)
      ) {
        return true;
      }
    }
    return false;
  }

  function expectStable(pts: Point[]) {
    expect(enforce(storableWaypoints(pts))).toEqual(pts);
    expect(enforcePerpendicularEnds(pts, "bottom", "left", LEG, CHARLIE)).toEqual(pts);
  }

  it("turns after the user's last pin, not at the source approach (finding 1)", () => {
    // Pins on the straight run down: collinear, but still the user's intent.
    const pts = enforce([{ x: 240, y: 300 }, { x: 240, y: 400 }]);
    expect(pts).toEqual([
      src,
      { x: 240, y: 235 },
      { x: 240, y: 400 },
      { x: 81, y: 400 },
      { x: 81, y: 498 },
      tgt,
    ]);
    expect(throughCard(pts)).toBe(false);
    expectStable(pts);
  });

  it("keeps a corridor dragged BELOW the card where it was put (finding 2)", () => {
    // The y=400 corridor dragged 140px down, past Charlie.
    const pts = enforce([{ x: 240, y: 540 }, { x: 81, y: 540 }]);
    expect(pts).toEqual([
      src,
      { x: 240, y: 235 },
      { x: 341, y: 235 },
      { x: 341, y: 540 },
      { x: 81, y: 540 },
      { x: 81, y: 498 },
      tgt,
    ]);
    expect(throughCard(pts)).toBe(false);
    expect(orthogonal(pts)).toBe(true);
    expectStable(pts);
  });

  it("goes round by the far side when the near one would swallow the dragged run", () => {
    // Vertical nearer the card's LEFT border: going round on the left lands on
    // the approach column and drops the run at y=540 — so round the right.
    const pts = enforcePerpendicularEnds(
      [{ x: 160, y: 195 }, { x: 160, y: 540 }, { x: 81, y: 540 }, tgt],
      "bottom",
      "left",
      LEG,
      CHARLIE,
    );
    expect(pts).toContainEqual({ x: 81, y: 540 });
    expect(pts.some((p) => p.y === 540 && p.x > CHARLIE.x + CHARLIE.width)).toBe(true);
    expect(throughCard(pts)).toBe(false);
    expect(orthogonal(pts)).toBe(true);
  });
});

describe("enforcePerpendicularEnds — the sidestep lane (#844 FP iter-5, finding 3)", () => {
  const CHARLIE = { x: 121, y: 481, width: 160, height: 35 };
  const right = CHARLIE.x + CHARLIE.width;

  it("detours a leg and a half out, off the lane other edges' legs run on", () => {
    const pts = enforcePerpendicularEnds(
      [{ x: 240, y: 195 }, { x: 240, y: 540 }, { x: 81, y: 540 }, { x: 121, y: 498 }],
      "bottom",
      "left",
      LEG,
      CHARLIE,
    );
    const columns = pts.slice(1, -1).map((p) => p.x);
    // One leg beyond the right border is where an edge landing on Charlie's
    // right side runs its perpendicular leg (the reported overlap at x=320).
    expect(columns).not.toContain(right + LEG);
    expect(columns).toContain(right + LEG * 1.5);
  });
});

describe("enforcePerpendicularEnds — an AUTO route around its target (#844 FP iter-5, finding 1)", () => {
  // simple-bugfix `else`: Visual tester's bottom output to « implementer »,
  // above-left of it, landing on its default left side. The router planned the
  // wire as leaving rightwards, which leaves a collinear bend at the source's
  // bottom border (1160,406) once the source leg is imposed.
  const IMPLEMENTER = { x: 1109, y: -19, width: 160, height: 35 };
  const src = { x: 1232, y: 406 };
  const tgt = { x: 1110, y: -1 };
  const snapped = [src, { x: 1160, y: 406 }, { x: 1160, y: -1 }, tgt];

  it("re-lays from its last real corner, not from a router bend on its own border", () => {
    const pts = enforcePerpendicularEnds(snapped, "bottom", "left", LEG, IMPLEMENTER, {
      manual: false,
    });
    expect(pts).toEqual([src, { x: 1232, y: 446 }, { x: 1070, y: 446 }, { x: 1070, y: -1 }, tgt]);
    // Nothing runs along the source card's bottom border (y=406) any more.
    expect(pts.slice(1).some((p) => p.y === 406)).toBe(false);
  });

  it("is a fixed point once stored", () => {
    const once = enforcePerpendicularEnds(snapped, "bottom", "left", LEG, IMPLEMENTER, {
      manual: false,
    });
    expect(
      enforcePerpendicularEnds(once, "bottom", "left", LEG, IMPLEMENTER, { manual: false }),
    ).toEqual(once);
  });

  it("goes under, not over, on the Verdict = Pass edge to « Ship It »", () => {
    const SHIP = { x: 748, y: 399, width: 160, height: 35 };
    const shipTgt = { x: 749, y: 417 };
    const pts = enforcePerpendicularEnds(
      [src, { x: 1000, y: 406 }, { x: 1000, y: 417 }, shipTgt],
      "bottom",
      "left",
      LEG,
      SHIP,
      { manual: false },
    );
    expect(pts).toEqual([
      src,
      { x: 1232, y: 446 },
      { x: 1000, y: 446 },
      { x: 1000, y: 474 },
      { x: 709, y: 474 },
      { x: 709, y: 417 },
      shipTgt,
    ]);
  });
});

describe("dragSegmentKeepingRun (#844 FP iter-5, finding 2)", () => {
  // Charlie's top border ON the lattice line y=480, as on the FP canvas.
  const CHARLIE = { x: 121, y: 480, width: 160, height: 36 };
  const src = { x: 240, y: 195 };
  const tgt = { x: 121, y: 498 };
  const grid = { origin: { x: 0, y: 0 }, step: GRID_STEP, free: false };
  const enforce = (pts: Point[]) => enforcePerpendicularEnds(pts, "bottom", "left", LEG, CHARLIE);
  // …240,235 → 240,400 → 81,400 → 81,498…: segment 2 is the corridor at y=400.
  const route = enforce([src, { x: 240, y: 400 }, tgt]);

  /** The y of the dragged corridor in an enforced route: its lowest run left of x=240. */
  const corridorY = (pts: Point[]) =>
    Math.max(...pts.slice(2, -2).filter((p) => p.x === 81).map((p) => p.y).filter((y) => y !== 498));

  it("never jumps back above the card while the pointer crosses the card's band", () => {
    let held = route;
    const seen: number[] = [];
    for (let y = 400; y <= 560; y += 4) {
      const next = dragSegmentKeepingRun(route, 2, y, grid, enforce, CHARLIE);
      if (next.keepsRun) held = next.enforced;
      seen.push(corridorY(held));
    }
    // Follows the pointer down, and only down.
    for (let i = 1; i < seen.length; i++) expect(seen[i]).toBeGreaterThanOrEqual(seen[i - 1]);
    expect(seen[seen.length - 1]).toBe(560);
  });

  it("refuses the magnet onto the landing approach, which parks the run across the card", () => {
    // 498 is the landing approach's y, within half a cell of the pointer.
    const next = dragSegmentKeepingRun(route, 2, 506, grid, enforce, CHARLIE);
    expect(next.keepsRun).toBe(true);
    expect(next.moved[2].y).toBe(520);
  });

  it("does not count a run laid along the card's border as kept", () => {
    const next = dragSegmentKeepingRun(route, 2, 484, grid, enforce, CHARLIE);
    expect(next.keepsRun).toBe(false);
  });

  it("changes nothing away from the card", () => {
    const next = dragSegmentKeepingRun(route, 2, 441, grid, enforce, CHARLIE);
    expect(next.keepsRun).toBe(true);
    expect(next.enforced).toEqual(enforce([src, { x: 240, y: 440 }, { x: 81, y: 440 }, tgt]));
  });
});

describe("storableWaypoints", () => {
  it("stores only the points BETWEEN the two legs — the legs are re-derived", () => {
    const enforced = [
      { x: 0, y: 0 },
      { x: 0, y: 40 }, // source approach
      { x: 100, y: 40 },
      { x: 100, y: 200 }, // target approach
      { x: 140, y: 200 },
    ];
    expect(storableWaypoints(enforced)).toEqual([{ x: 100, y: 40 }]);
  });

  it("stores nothing at all when the path IS its two legs", () => {
    expect(
      storableWaypoints([
        { x: 0, y: 0 },
        { x: 0, y: 40 },
        { x: 100, y: 40 },
        { x: 100, y: 80 },
      ]),
    ).toEqual([]);
  });

  it("round-trips: enforcing a stored route reproduces the drawn one", () => {
    const drawn = enforcePerpendicularEnds(
      [
        { x: 160, y: 180 },
        { x: 160, y: 400 },
        { x: 320, y: 400 },
        { x: 500, y: 300 },
      ],
      "bottom",
      "left",
      LEG,
    );
    const reloaded = enforcePerpendicularEnds(
      [drawn[0], ...storableWaypoints(drawn), drawn[drawn.length - 1]],
      "bottom",
      "left",
      LEG,
    );
    expect(reloaded).toEqual(drawn);
  });
});

describe("clipOutside", () => {
  it("drops the trailing points already inside the target's neighbourhood", () => {
    const pts = [
      { x: 0, y: 0 },
      { x: 0, y: 140 },
      { x: 90, y: 140 }, // within one leg of RECT's left border
      { x: 150, y: 140 }, // on the card
    ];
    expect(clipOutside(pts, RECT, LEG)).toEqual([
      { x: 0, y: 0 },
      { x: 0, y: 140 },
    ]);
  });

  it("keeps at least the departure point", () => {
    expect(clipOutside([{ x: 150, y: 140 }], RECT, LEG)).toHaveLength(1);
  });
});

describe("approachPoint", () => {
  it("steps one leg OUT of the card, away from the side", () => {
    expect(approachPoint({ x: 100, y: 140 }, "left", LEG)).toEqual({ x: 100 - LEG, y: 140 });
    expect(approachPoint({ x: 300, y: 140 }, "right", LEG)).toEqual({ x: 300 + LEG, y: 140 });
    expect(approachPoint({ x: 160, y: 100 }, "top", LEG)).toEqual({ x: 160, y: 100 - LEG });
    expect(approachPoint({ x: 160, y: 180 }, "bottom", LEG)).toEqual({ x: 160, y: 180 + LEG });
  });

  it("is one grid cell, with a 16px floor", () => {
    expect(landingLeg(40)).toBe(40);
    expect(landingLeg(8)).toBe(16);
  });
});
