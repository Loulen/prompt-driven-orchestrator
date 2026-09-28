import { describe, it, expect } from "vitest";
import { shouldClearTriggerOnCanvasFocus } from "./triggerCanvasReconcile";
import type { TriggerCanvasFocusInputs } from "./triggerCanvasReconcile";

// A Trigger is selected and it opened pipeline "pipe-a" in the canvas — the
// baseline #320 state. Individual tests override the fields that matter.
const base: TriggerCanvasFocusInputs = {
  selectedTriggerId: "trig-1",
  editActiveTabId: "pipe-a",
  selectionKind: "none",
  triggerOpenedTabId: "pipe-a",
};

describe("shouldClearTriggerOnCanvasFocus (#320)", () => {
  it("keeps the Trigger on its own openPipeline landing (fresh tab)", () => {
    // editActiveTabId === triggerOpenedTabId, selection none → the Trigger's
    // own open, not a reclaim.
    expect(shouldClearTriggerOnCanvasFocus(base)).toBe(false);
  });

  it("keeps the Trigger when its pipeline was already the active tab", () => {
    // The already-open branch of openPipeline resets selection to none without
    // changing activeTabId — the ref still equals the active tab.
    expect(
      shouldClearTriggerOnCanvasFocus({ ...base, selectionKind: "none" }),
    ).toBe(false);
  });

  it("clears on a node selection (a genuine #247 reclaim)", () => {
    expect(
      shouldClearTriggerOnCanvasFocus({ ...base, selectionKind: "node" }),
    ).toBe(true);
  });

  it("clears on an edge selection", () => {
    expect(
      shouldClearTriggerOnCanvasFocus({ ...base, selectionKind: "edge" }),
    ).toBe(true);
  });

  it("clears on a region selection", () => {
    expect(
      shouldClearTriggerOnCanvasFocus({ ...base, selectionKind: "region" }),
    ).toBe(true);
  });

  it("clears when switching to / opening a different tab (tabId !== ref)", () => {
    expect(
      shouldClearTriggerOnCanvasFocus({ ...base, editActiveTabId: "pipe-b" }),
    ).toBe(true);
  });

  it("clears when a focus change lands with no trigger-opened tab recorded", () => {
    // No Trigger-opened tab (ref null) but a Trigger is somehow selected and the
    // canvas focus changed → treat as a reclaim and clear.
    expect(
      shouldClearTriggerOnCanvasFocus({ ...base, triggerOpenedTabId: null }),
    ).toBe(true);
  });

  it("is a no-op when no Trigger is selected (never clears what isn't there)", () => {
    expect(
      shouldClearTriggerOnCanvasFocus({ ...base, selectedTriggerId: null }),
    ).toBe(false);
    // Even on an obvious reclaim shape, a null selection stays a no-op.
    expect(
      shouldClearTriggerOnCanvasFocus({
        selectedTriggerId: null,
        editActiveTabId: "pipe-b",
        selectionKind: "node",
        triggerOpenedTabId: "pipe-a",
      }),
    ).toBe(false);
  });

  it("still keeps the Trigger on a deselect-to-empty on its own tab (not a reclaim)", () => {
    // Explicitly pinning the #320 edge case: blank-canvas click leaves kind
    // "none" on the Trigger's own tab — keep.
    expect(
      shouldClearTriggerOnCanvasFocus({
        selectedTriggerId: "trig-1",
        editActiveTabId: "pipe-a",
        selectionKind: "none",
        triggerOpenedTabId: "pipe-a",
      }),
    ).toBe(false);
  });
});
