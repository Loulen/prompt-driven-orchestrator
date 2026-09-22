import { describe, it, expect } from "vitest";
import type { Point } from "./orthogonalRouter";
import {
  anchorFromPoint,
  anchorPoint,
  approachPoint,
  clipOutside,
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
import { isAxisAligned, WIRING_GRID_STEP } from "./wiringGrid";

// A 200x80 card whose top-left is at (100, 100).
const RECT = { x: 100, y: 100, width: 200, height: 80 };
const LEG = landingLeg(WIRING_GRID_STEP);

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
    const a = anchorFromPoint({ x: 173, y: 400 }, RECT, "bottom", WIRING_GRID_STEP);
    expect(a.side).toBe("bottom");
    // 173 snaps to 160 in absolute flow coordinates; the offset is relative.
    expect(a.offset).toBe(60);
    expect(anchorPoint(RECT, a, a.side).x % WIRING_GRID_STEP).toBe(0);
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
