import { describe, it, expect } from "vitest";
import { shouldAutoSnapToLiveNode } from "./autoSnap";
import type { SelectionKind } from "../stores/editStore";
import type { RunStatus } from "../types";

describe("shouldAutoSnapToLiveNode", () => {
  it("snaps on a running/awaiting run with nothing selected", () => {
    expect(shouldAutoSnapToLiveNode("none", false, "running")).toBe(true);
    expect(shouldAutoSnapToLiveNode("none", false, "awaiting_user")).toBe(true);
  });

  it("does not snap when a node is already selected", () => {
    expect(shouldAutoSnapToLiveNode("node", true, "running")).toBe(false);
  });

  // A "node" selection with no id is a degenerate state; treat it like nothing
  // selected so the snap can recover the terminal.
  it("snaps for a node selection missing its id on a live run", () => {
    expect(shouldAutoSnapToLiveNode("node", false, "running")).toBe(true);
  });

  it("yields to an explicit inspector selection (#150 / #147 / #307)", () => {
    for (const kind of ["region", "edge", "note"] as SelectionKind[]) {
      expect(shouldAutoSnapToLiveNode(kind, false, "running")).toBe(false);
    }
  });

  // The load-bearing F1 case: opening the Run-info / Repositories sidebar must
  // survive the auto-snap, or it would be unreachable while a node runs.
  it("never steals the pane back from the Run-info sidebar (#465 slice 2, F1)", () => {
    expect(shouldAutoSnapToLiveNode("run", false, "running")).toBe(false);
    expect(shouldAutoSnapToLiveNode("run", false, "awaiting_user")).toBe(false);
  });

  it("does not snap on a paused run — its sidebar is reachable by deselecting", () => {
    expect(shouldAutoSnapToLiveNode("none", false, "paused")).toBe(false);
  });

  it("does not snap on a terminal run", () => {
    for (const status of [
      "completed",
      "failed",
      "skipped",
      "halted",
      "archived",
    ] as RunStatus[]) {
      expect(shouldAutoSnapToLiveNode("none", false, status)).toBe(false);
    }
  });
});
