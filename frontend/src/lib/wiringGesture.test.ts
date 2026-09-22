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
import { enforcePerpendicularEnds, landingLeg } from "./anchorSide";

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

describe("landingAt", () => {
  it("takes the side aimed at and the position along it, per pixel", () => {
    expect(landingAt({ x: 470, y: 412 }, TGT_RECT).anchor).toEqual({ side: "top", offset: 70 });
    expect(landingAt({ x: 408, y: 450 }, TGT_RECT).anchor).toEqual({ side: "left", offset: 50 });
  });
});
