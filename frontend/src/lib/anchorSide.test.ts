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
