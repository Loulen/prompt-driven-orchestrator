import { describe, it, expect, beforeEach } from "vitest";
import { useSelectionStore } from "./selectionStore";

const s = () => useSelectionStore.getState();

beforeEach(() => {
  useSelectionStore.getState().clearAll();
});

describe("selectionStore", () => {
  it("toggles membership and records the anchor", () => {
    s().toggle("runs", "a");
    expect(s().runs).toEqual(["a"]);
    expect(s().anchor.runs).toBe("a");
    s().toggle("runs", "b");
    expect(s().runs).toEqual(["a", "b"]);
    expect(s().anchor.runs).toBe("b");
    // toggling an already-selected id removes it
    s().toggle("runs", "a");
    expect(s().runs).toEqual(["b"]);
    expect(s().anchor.runs).toBe("a");
  });

  it("keeps the three tabs independent", () => {
    s().toggle("runs", "a");
    s().toggle("triggers", "t1");
    s().toggle("library", "repo-lib1");
    expect(s().runs).toEqual(["a"]);
    expect(s().triggers).toEqual(["t1"]);
    expect(s().library).toEqual(["repo-lib1"]);
    s().clear("runs");
    // clearing one tab leaves the others intact — this is what persists a badge
    expect(s().runs).toEqual([]);
    expect(s().triggers).toEqual(["t1"]);
    expect(s().library).toEqual(["repo-lib1"]);
  });

  it("selectRange extends a contiguous range from the anchor (union)", () => {
    const ordered = ["a", "b", "c", "d", "e"];
    s().toggle("runs", "b"); // anchor = b
    s().selectRange("runs", "d", ordered);
    expect(s().runs).toEqual(["b", "c", "d"]);
    // anchor stays put — a second shift-click extends from the same origin
    s().selectRange("runs", "a", ordered);
    expect(new Set(s().runs)).toEqual(new Set(["a", "b", "c", "d"]));
  });

  it("selectRange with no usable anchor falls back to a plain select", () => {
    const ordered = ["a", "b", "c"];
    s().selectRange("runs", "b", ordered); // no anchor yet
    expect(s().runs).toEqual(["b"]);
    expect(s().anchor.runs).toBe("b");
  });

  it("selectVisible unions every visible id", () => {
    s().toggle("runs", "a");
    s().selectVisible("runs", ["a", "b", "c"]);
    expect(new Set(s().runs)).toEqual(new Set(["a", "b", "c"]));
  });

  it("selectGroup toggles the whole group", () => {
    // first call selects the group
    s().selectGroup("runs", ["a", "b"]);
    expect(new Set(s().runs)).toEqual(new Set(["a", "b"]));
    // second call (all already selected) clears exactly the group, keeps others
    s().toggle("runs", "z");
    s().selectGroup("runs", ["a", "b"]);
    expect(s().runs).toEqual(["z"]);
  });

  it("deselect drops only the named ids", () => {
    s().selectVisible("runs", ["a", "b", "c"]);
    s().deselect("runs", ["a", "c"]);
    expect(s().runs).toEqual(["b"]);
  });

  it("clearAll empties every tab and every anchor", () => {
    s().toggle("runs", "a");
    s().toggle("triggers", "t");
    s().toggle("library", "l");
    s().clearAll();
    expect(s().runs).toEqual([]);
    expect(s().triggers).toEqual([]);
    expect(s().library).toEqual([]);
    expect(s().anchor).toEqual({ runs: null, triggers: null, library: null });
  });
});
