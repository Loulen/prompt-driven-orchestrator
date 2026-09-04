import { describe, it, expect } from "vitest";
import { isStructuralMarker, resolveNodeInspector } from "./structuralMarkers";

describe("isStructuralMarker (#684)", () => {
  it("is true for start and end, by type or by node", () => {
    expect(isStructuralMarker("start")).toBe(true);
    expect(isStructuralMarker("end")).toBe(true);
    expect(isStructuralMarker({ type: "start" })).toBe(true);
    expect(isStructuralMarker({ type: "end" })).toBe(true);
  });

  it("is false for every other node type and for a missing node", () => {
    expect(isStructuralMarker("agent")).toBe(false);
    expect(isStructuralMarker("merge")).toBe(false);
    expect(isStructuralMarker("script")).toBe(false);
    expect(isStructuralMarker(null)).toBe(false);
    expect(isStructuralMarker(undefined)).toBe(false);
  });
});

describe("resolveNodeInspector (#684)", () => {
  const base = { isEditingRun: false, hasRunStart: false, hasRunEnd: false };

  it("routes agent / merge / script nodes to the full node inspector", () => {
    expect(resolveNodeInspector({ ...base, nodeType: "agent" })).toBe("node");
    expect(resolveNodeInspector({ ...base, nodeType: "merge" })).toBe("node");
    expect(resolveNodeInspector({ ...base, nodeType: "script" })).toBe("node");
  });

  it("shows the read-only marker pane for start/end outside a run (the bug)", () => {
    expect(resolveNodeInspector({ ...base, nodeType: "start" })).toBe("marker");
    expect(resolveNodeInspector({ ...base, nodeType: "end" })).toBe("marker");
  });

  it("shows the runtime StartInspector / EndInspector inside a run", () => {
    expect(resolveNodeInspector({ nodeType: "start", isEditingRun: true, hasRunStart: true, hasRunEnd: true }))
      .toBe("run-start");
    expect(resolveNodeInspector({ nodeType: "end", isEditingRun: true, hasRunStart: true, hasRunEnd: true }))
      .toBe("run-end");
  });

  it("never falls back to the generic editor when run info is missing", () => {
    expect(resolveNodeInspector({ nodeType: "start", isEditingRun: true, hasRunStart: false, hasRunEnd: false }))
      .toBe("marker");
    expect(resolveNodeInspector({ nodeType: "end", isEditingRun: true, hasRunStart: true, hasRunEnd: false }))
      .toBe("marker");
    // A run tab's end marker is not confused with the start info being present.
    expect(resolveNodeInspector({ nodeType: "end", isEditingRun: true, hasRunStart: true, hasRunEnd: false }))
      .not.toBe("run-start");
  });
});
