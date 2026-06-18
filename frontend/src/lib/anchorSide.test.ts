import { describe, it, expect } from "vitest";
import {
  chooseAnchorSide,
  anchorHandleId,
  sideFromAnchorHandle,
  anchorsByDropOnBody,
} from "./anchorSide";

// A 200x80 card whose top-left is at (100, 100); centre is (200, 140).
const RECT = { x: 100, y: 100, width: 200, height: 80 };

describe("chooseAnchorSide", () => {
  it("picks the side of the target card nearest the drop point", () => {
    // Drop just outside the left edge, vertically centred.
    expect(chooseAnchorSide({ x: 90, y: 140 }, RECT)).toBe("left");
  });

  it("anchors right when the drop is near the right edge", () => {
    expect(chooseAnchorSide({ x: 310, y: 140 }, RECT)).toBe("right");
  });

  it("anchors top when the drop is near the top edge", () => {
    expect(chooseAnchorSide({ x: 200, y: 92 }, RECT)).toBe("top");
  });

  it("anchors bottom when the drop is near the bottom edge", () => {
    expect(chooseAnchorSide({ x: 200, y: 188 }, RECT)).toBe("bottom");
  });

  it("keeps the legacy left default for a dead-centre drop", () => {
    expect(chooseAnchorSide({ x: 200, y: 140 }, RECT)).toBe("left");
  });

  it("anchors by the side the drop lands on INSIDE the body, not only outside it", () => {
    // The reporter drops on the node body (#219), not just past its edges.
    expect(chooseAnchorSide({ x: 120, y: 140 }, RECT)).toBe("left"); // left interior
    expect(chooseAnchorSide({ x: 280, y: 140 }, RECT)).toBe("right"); // right interior
    expect(chooseAnchorSide({ x: 200, y: 110 }, RECT)).toBe("top"); // upper interior
    expect(chooseAnchorSide({ x: 200, y: 170 }, RECT)).toBe("bottom"); // lower interior
  });
});

describe("chooseAnchorSide — short, wide card (#219 regression)", () => {
  // The default work node is short and wide (~160x35). The old
  // perpendicular-distance-to-each-edge metric mis-resolved interior drops here:
  // because the card is so short, almost any body drop sat nearer the top/bottom
  // edges and spuriously anchored top/bottom. Normalising by half-extents fixes it.
  const WIDE = { x: 0, y: 0, width: 160, height: 35 };

  it("anchors LEFT for a drop in the left portion of the body (not top/bottom)", () => {
    // Vertically centred, ~25% across. Old metric: top/bottom (17.5) beat left (40).
    expect(chooseAnchorSide({ x: 40, y: 17 }, WIDE)).toBe("left");
  });

  it("anchors RIGHT for a drop in the right portion of the body (not top/bottom)", () => {
    expect(chooseAnchorSide({ x: 130, y: 18 }, WIDE)).toBe("right");
  });

  it("keeps left as the centre default on a wide card (no spurious top/bottom)", () => {
    expect(chooseAnchorSide({ x: 80, y: 17.5 }, WIDE)).toBe("left");
  });

  it("still anchors TOP when the drop genuinely aims at the top edge", () => {
    expect(chooseAnchorSide({ x: 80, y: 2 }, WIDE)).toBe("top");
  });

  it("still anchors BOTTOM when the drop genuinely aims at the bottom edge", () => {
    expect(chooseAnchorSide({ x: 80, y: 33 }, WIDE)).toBe("bottom");
  });

  it("prefers the horizontal side near a wide card's corner (aspect-ratio-aware)", () => {
    // Near the bottom-right corner of a wide card the user is aiming at the long
    // right side, not the short bottom one: |dx| (0.875) > |dy| (0.71) -> right.
    expect(chooseAnchorSide({ x: 150, y: 30 }, WIDE)).toBe("right");
  });
});

describe("anchorHandleId / sideFromAnchorHandle round-trip", () => {
  it("round-trips every side through its body-handle id", () => {
    for (const side of ["left", "right", "top", "bottom"] as const) {
      expect(sideFromAnchorHandle(anchorHandleId(side))).toBe(side);
    }
  });

  it("returns null for a declared port name or a missing handle", () => {
    expect(sideFromAnchorHandle("result")).toBeNull();
    expect(sideFromAnchorHandle(null)).toBeNull();
    expect(sideFromAnchorHandle(undefined)).toBeNull();
  });
});

describe("anchorsByDropOnBody", () => {
  it("anchors by drop when the edge landed on an emergent body handle", () => {
    expect(anchorsByDropOnBody(anchorHandleId("right"))).toBe(true);
  });

  it("does not anchor when the edge landed on a declared/structural port", () => {
    // End's `result`, a merge `branches` handle, etc. keep their fixed side.
    expect(anchorsByDropOnBody("result")).toBe(false);
    expect(anchorsByDropOnBody("branches")).toBe(false);
    expect(anchorsByDropOnBody(null)).toBe(false);
  });
});
