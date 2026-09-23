import { describe, it, expect } from "vitest";
import type { Point } from "./orthogonalRouter";
import {
  advanceGesture,
  gesturePath,
  landingAt,
  startGesture,
  type WiringGesture,
} from "./wiringGesture";
import { isAxisAligned, WIRING_GRID_STEP } from "./wiringGrid";
import { enforcePerpendicularEnds, landingConnector, landingLeg } from "./anchorSide";

const STEP = WIRING_GRID_STEP;
const LEG = landingLeg(STEP);
// The wire leaves the bottom of a card whose bottom border is at y = 80.
const FROM: Point = { x: 80, y: 80 };
// A target card well below and to the right.
const TGT_RECT = { x: 400, y: 400, width: 200, height: 80 };

function orthogonal(points: Point[]): boolean {
  for (let i = 1; i < points.length; i++) {
    if (!isAxisAligned(points[i - 1], points[i])) return false;
  }
  return true;
}

/**
 * The segments of a path that run through a card's inside — the landing leg
 * excepted, which is the one segment that is SUPPOSED to touch the border.
 *
 * Stronger than « no waypoint inside the card »: the spur the FP found crossed
 * the whole body with both of its ends outside it.
 */
function crossings(
  points: Point[],
  rect: { x: number; y: number; width: number; height: number },
): [Point, Point][] {
  const out: [Point, Point][] = [];
  for (let i = 1; i < points.length - 1; i++) {
    const [a, b] = [points[i - 1], points[i]];
    const spans = (lo: number, hi: number, e0: number, e1: number) =>
      Math.min(lo, hi) < e1 && Math.max(lo, hi) > e0;
    if (
      spans(a.x, b.x, rect.x, rect.x + rect.width) &&
      spans(a.y, b.y, rect.y, rect.y + rect.height)
    ) {
      out.push([a, b]);
    }
  }
  return out;
}

/** Walk the gesture through a list of frames. */
function walk(
  g: WiringGesture,
  frames: { cursor: Point; shift?: boolean; overTarget?: boolean }[],
): WiringGesture {
  return frames.reduce(
    (acc, f) =>
      advanceGesture(acc, {
        cursor: f.cursor,
        shift: f.shift ?? false,
        overTarget: f.overTarget ?? false,
        step: STEP,
      }),
    g,
  );
}

describe("startGesture", () => {
  it("seeds the perpendicular leg out of the pressed side", () => {
    const g = startGesture(FROM, "bottom", STEP);
    expect(g.trace.points).toEqual([FROM, { x: FROM.x, y: FROM.y + LEG }]);
    expect(g.trace.heading).toBe("vertical");
    expect(g.origin).toEqual(FROM);
  });

  it("leaves the card outward even when the cursor is dragged back over it", () => {
    // Dragging back over the source card lets the tip slide past the departure
    // point, so the live trace alone can point inwards. What is PERSISTED must
    // not: enforcement re-inserts the outward leg, so the saved wire always
    // leaves the border before going anywhere.
    const g = walk(startGesture(FROM, "bottom", STEP), [{ cursor: { x: 80, y: 0 } }]);
    expect(orthogonal(g.trace.points)).toBe(true);
    const enforced = enforcePerpendicularEnds(
      [...g.trace.points, { x: 470, y: 400 }],
      "bottom",
      "top",
      LEG,
    );
    expect(enforced[0]).toEqual(FROM);
    expect(enforced[1]).toEqual({ x: FROM.x, y: FROM.y + LEG });
  });
});

describe("advanceGesture — the progressive trace", () => {
  it("adds one orthogonal segment per crossed cell, all of it on the lattice", () => {
    const g = walk(startGesture(FROM, "bottom", STEP), [
      { cursor: { x: 85, y: 200 } },
      { cursor: { x: 90, y: 280 } },
      { cursor: { x: 210, y: 285 } },
    ]);
    expect(orthogonal(g.trace.points)).toBe(true);
    for (const p of g.trace.points.slice(1)) {
      expect((p.x - g.origin.x) % STEP).toBe(0);
      expect((p.y - g.origin.y) % STEP).toBe(0);
    }
  });

  it("un-draws a bend when the gesture walks back over it", () => {
    const forward = walk(startGesture(FROM, "bottom", STEP), [
      { cursor: { x: 80, y: 280 } },
      { cursor: { x: 240, y: 280 } },
    ]);
    const back = walk(forward, [{ cursor: { x: 80, y: 280 } }]);
    expect(back.trace.points.length).toBeLessThan(forward.trace.points.length);
  });
});

describe("advanceGesture — Shift", () => {
  it("commits nothing while Shift is down; the pending L only follows the cursor", () => {
    const before = walk(startGesture(FROM, "bottom", STEP), [{ cursor: { x: 80, y: 280 } }]);
    const held = walk(before, [{ cursor: { x: 213, y: 357 }, shift: true }]);
    expect(held.trace.points).toEqual(before.trace.points);
    expect(held.shift).toBe(true);
    // …but the DRAWN path does show the free continuation, as an orthogonal L.
    const drawn = gesturePath(held, null, STEP);
    expect(orthogonal(drawn)).toBe(true);
    expect(drawn[drawn.length - 1]).toEqual({ x: 213, y: 357 });
    // Never a staircase: exactly one extra bend at most.
    expect(drawn.length).toBeLessThanOrEqual(before.trace.points.length + 2);
  });

  it("keeps the freehand run verbatim and re-origins the lattice on its tip", () => {
    const before = walk(startGesture(FROM, "bottom", STEP), [{ cursor: { x: 80, y: 280 } }]);
    const held = walk(before, [{ cursor: { x: 213, y: 357 }, shift: true }]);
    const released = walk(held, [{ cursor: { x: 213, y: 357 } }]);

    // The freehand tip survived as a committed point…
    expect(released.trace.points).toContainEqual({ x: 213, y: 357 });
    // …and the lattice now passes through it.
    expect(released.origin).toEqual({ x: 213, y: 357 });
    expect(released.shift).toBe(false);

    // What is drawn next lines up with what was just drawn, not with where the
    // gesture started.
    const next = walk(released, [{ cursor: { x: 400, y: 500 } }]);
    for (const p of next.trace.points.slice(-2)) {
      expect((p.x - next.origin.x) % STEP).toBe(0);
      expect((p.y - next.origin.y) % STEP).toBe(0);
    }
    expect(orthogonal(next.trace.points)).toBe(true);
  });
});

describe("advanceGesture — over the target", () => {
  it("freezes the trace: nothing on the grid is committed inside the card", () => {
    const before = walk(startGesture(FROM, "bottom", STEP), [{ cursor: { x: 80, y: 280 } }]);
    const over = walk(before, [{ cursor: { x: 470, y: 420 }, overTarget: true }]);
    expect(over.trace.points).toEqual(before.trace.points);
    expect(over.cursor).toEqual({ x: 470, y: 420 });
  });
});

describe("gesturePath — landing", () => {
  it("ends on the anchor, entering the card perpendicular to the side aimed at", () => {
    const g = walk(startGesture(FROM, "bottom", STEP), [
      { cursor: { x: 80, y: 280 } },
      { cursor: { x: 470, y: 285 } },
    ]);
    const landing = landingAt({ x: 470, y: 412 }, TGT_RECT);
    expect(landing.anchor.side).toBe("top");
    const path = gesturePath(g, landing, STEP);
    expect(orthogonal(path)).toBe(true);
    expect(path[path.length - 1]).toEqual(landing.point);
    const before = path[path.length - 2];
    // Vertical, downwards, at least one leg long: the arrowhead enters straight.
    expect(before.x).toBe(landing.point.x);
    expect(landing.point.y - before.y).toBeGreaterThanOrEqual(LEG);
  });

  it("clips a trace that already stepped onto the card, so no spur is drawn", () => {
    // xyflow reports the hover a frame late; the tip is already on the border.
    const g = walk(startGesture(FROM, "bottom", STEP), [
      { cursor: { x: 80, y: 280 } },
      { cursor: { x: 470, y: 285 } },
      { cursor: { x: 470, y: 405 } },
    ]);
    const landing = landingAt({ x: 470, y: 412 }, TGT_RECT);
    const path = gesturePath(g, landing, STEP);
    // The wire never enters the card, leaves it, and comes back.
    const insideCount = path.filter(
      (p) =>
        p.x >= TGT_RECT.x &&
        p.x <= TGT_RECT.x + TGT_RECT.width &&
        p.y > TGT_RECT.y &&
        p.y <= TGT_RECT.y + TGT_RECT.height,
    ).length;
    expect(insideCount).toBe(0);
    expect(orthogonal(path)).toBe(true);
  });
});

describe("gesturePath — landing on the far side of the card (FP finding 2)", () => {
  // Aiming at the BOTTOM of a card the wire reaches from above. The approach point
  // is one leg below the card, so the naive L walks the whole card body, overshoots
  // and doubles back in — invisible under an opaque card, and persisted.
  const overshoot = walk(startGesture(FROM, "bottom", STEP), [
    { cursor: { x: 80, y: 280 } },
    { cursor: { x: 470, y: 285 } },
  ]);

  it("goes AROUND the card instead of through it", () => {
    const landing = landingAt({ x: 470, y: 470 }, TGT_RECT);
    expect(landing.anchor.side).toBe("bottom");
    const path = gesturePath(overshoot, landing, STEP);
    expect(orthogonal(path)).toBe(true);
    expect(crossings(path, TGT_RECT)).toEqual([]);
  });

  it("still enters perpendicular, and still ends on the anchor", () => {
    const landing = landingAt({ x: 470, y: 470 }, TGT_RECT);
    const path = gesturePath(overshoot, landing, STEP);
    const [before, last] = path.slice(-2);
    expect(last).toEqual(landing.point);
    expect(before.x).toBe(landing.point.x);
    // Entering the bottom border means travelling UPWARDS into it.
    expect(before.y - last.y).toBeGreaterThanOrEqual(LEG);
  });

  it("keeps the connector it always had when the landing is head-on", () => {
    // The detour is paid for only where it is needed: a landing the wire reaches
    // head-on still gets the plain L, rect or no rect.
    const landing = landingAt({ x: 470, y: 412 }, TGT_RECT);
    const tip = overshoot.trace.points[overshoot.trace.points.length - 1];
    expect(gesturePath(overshoot, landing, STEP)).toEqual([
      ...overshoot.trace.points.slice(0, -1),
      ...landingConnector(tip, landing.point, "top", LEG),
    ]);
  });
});

describe("landingAt", () => {
  it("takes the side aimed at and the position along it, per pixel", () => {
    expect(landingAt({ x: 470, y: 412 }, TGT_RECT).anchor).toEqual({ side: "top", offset: 70 });
    expect(landingAt({ x: 408, y: 450 }, TGT_RECT).anchor).toEqual({ side: "left", offset: 50 });
  });

  it("obeys a target that pins the wire to its own declared handle", () => {
    // A merge's `branches` (End lands by drop since #840): the drop position has
    // no say. The preview must land where the edge will actually be pinned, or it
    // draws — and persists — the approach to a border the wire never touches.
    const pin = { side: "top" as const, point: { x: 500, y: 400 } };
    const landing = landingAt({ x: 470, y: 470 }, TGT_RECT, pin);
    expect(landing.anchor.side).toBe("top");
    expect(landing.point).toEqual(pin.point);
  });

  it("falls back to the drop rule when nothing is pinned", () => {
    expect(landingAt({ x: 470, y: 470 }, TGT_RECT, null).anchor.side).toBe("bottom");
  });
});

describe("gesturePath — no spur where the trace hands over to the landing (#844 FP iter-2)", () => {
  it("drops the last cell when the connector runs straight back over it", () => {
    // The last cell was traced RIGHT to x=400, past the left side's approach at
    // x=360: the connector turns straight back LEFT over it. The preview kept
    // that 40px spur; the persisted route (collinear-merged) never did.
    const gesture: WiringGesture = {
      trace: { points: [{ x: 280, y: 560 }, { x: 400, y: 560 }], heading: "horizontal" },
      origin: { x: 0, y: 0 },
      cursor: { x: 405, y: 440 },
      shift: false,
      wasShift: false,
    };
    const landing = landingAt({ x: 405, y: 440 }, TGT_RECT);
    const path = gesturePath(gesture, landing, STEP);
    expect(path).toEqual([
      { x: 280, y: 560 },
      { x: 360, y: 560 },
      { x: 360, y: 440 },
      { x: 400, y: 440 },
    ]);
    // And since the drop persists this very polyline, the saved route has no
    // spur either.
    expect(orthogonal(path)).toBe(true);
  });
});
