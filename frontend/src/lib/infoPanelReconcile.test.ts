import { describe, it, expect } from "vitest";
import { shouldCloseInfoOnTabChange } from "./infoPanelReconcile";
import type { InfoPanelReconcileInputs } from "./infoPanelReconcile";

describe("shouldCloseInfoOnTabChange (#385)", () => {
  it("closes when the tab changed and the overlay is open", () => {
    expect(shouldCloseInfoOnTabChange({ prevTabId: "a", nextTabId: "b", infoOpen: true })).toBe(true);
  });

  it("stays open when the tab did not change (reselect same run/tab)", () => {
    expect(shouldCloseInfoOnTabChange({ prevTabId: "a", nextTabId: "a", infoOpen: true })).toBe(false);
  });

  it("no-op when the overlay is already closed", () => {
    expect(shouldCloseInfoOnTabChange({ prevTabId: "a", nextTabId: "b", infoOpen: false })).toBe(false);
  });

  // Two runs of the SAME pipeline still differ — run tabs are keyed
  // `__run__<runId>`. This is the make-or-break edge case for #385.
  it("closes when switching between two runs of the same pipeline", () => {
    expect(
      shouldCloseInfoOnTabChange({ prevTabId: "__run__A", nextTabId: "__run__B", infoOpen: true }),
    ).toBe(true);
  });

  // Full truth table — enumerate every combination (test-everything rule).
  it("matches the full truth table", () => {
    const cases: Array<[InfoPanelReconcileInputs, boolean]> = [
      [{ prevTabId: "a", nextTabId: "a", infoOpen: false }, false],
      [{ prevTabId: "a", nextTabId: "a", infoOpen: true }, false],
      [{ prevTabId: "a", nextTabId: "b", infoOpen: false }, false],
      [{ prevTabId: "a", nextTabId: "b", infoOpen: true }, true],
      [{ prevTabId: null, nextTabId: "__run__b", infoOpen: true }, true],
      [{ prevTabId: "a", nextTabId: null, infoOpen: true }, true],
      [{ prevTabId: null, nextTabId: null, infoOpen: true }, false],
      [{ prevTabId: null, nextTabId: null, infoOpen: false }, false],
    ];
    for (const [input, want] of cases) expect(shouldCloseInfoOnTabChange(input)).toBe(want);
  });
});
