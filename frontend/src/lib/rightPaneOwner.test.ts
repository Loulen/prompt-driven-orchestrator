import { describe, it, expect } from "vitest";
import { rightPaneOwner } from "./rightPaneOwner";
import type { RightPaneInputs } from "./rightPaneOwner";

const base: RightPaneInputs = {
  triggerSelected: false,
  infoPanelOpen: false,
  hasEditTab: false,
};

describe("rightPaneOwner", () => {
  it("falls back to the legacy node path when nothing is focused", () => {
    expect(rightPaneOwner(base)).toBe("selectedNode");
  });

  it("shows the edit-tab inspector when a tab owns the canvas", () => {
    expect(rightPaneOwner({ ...base, hasEditTab: true })).toBe("editTab");
  });

  it("shows the trigger detail when only a trigger is selected", () => {
    expect(rightPaneOwner({ ...base, triggerSelected: true })).toBe("trigger");
  });

  // The regression at the heart of #247: opening a run leaves a persistent edit
  // tab, so selecting a trigger afterwards must STILL surface its detail. The
  // old `!hasEditTab` guard routed this to "editTab" and blanked the pane.
  it("lets the trigger win over a persistent edit tab (#247)", () => {
    expect(
      rightPaneOwner({ triggerSelected: true, infoPanelOpen: false, hasEditTab: true }),
    ).toBe("trigger");
  });

  // #320: clicking a trigger opens its pipeline in the canvas (hasEditTab
  // becomes true) while the trigger detail must stay on the right. That is
  // exactly `triggerSelected && hasEditTab → "trigger"` — pin it against drift.
  it("keeps the trigger detail while its pipeline owns the canvas (#320)", () => {
    expect(
      rightPaneOwner({ triggerSelected: true, infoPanelOpen: false, hasEditTab: true }),
    ).toBe("trigger");
  });

  it("lets the info overlay win over a selected trigger", () => {
    expect(
      rightPaneOwner({ triggerSelected: true, infoPanelOpen: true, hasEditTab: false }),
    ).toBe("info");
  });

  it("lets the info overlay win over everything", () => {
    expect(
      rightPaneOwner({ triggerSelected: true, infoPanelOpen: true, hasEditTab: true }),
    ).toBe("info");
  });

  it("shows the edit tab when neither trigger nor info is active", () => {
    expect(
      rightPaneOwner({ triggerSelected: false, infoPanelOpen: false, hasEditTab: true }),
    ).toBe("editTab");
  });

  // Exhaustive truth table over the three boolean inputs, pinning the full
  // precedence (info > trigger > editTab > selectedNode) against drift.
  it("matches the full precedence truth table", () => {
    const expected: Array<[RightPaneInputs, string]> = [
      [{ triggerSelected: false, infoPanelOpen: false, hasEditTab: false }, "selectedNode"],
      [{ triggerSelected: false, infoPanelOpen: false, hasEditTab: true }, "editTab"],
      [{ triggerSelected: false, infoPanelOpen: true, hasEditTab: false }, "info"],
      [{ triggerSelected: false, infoPanelOpen: true, hasEditTab: true }, "info"],
      [{ triggerSelected: true, infoPanelOpen: false, hasEditTab: false }, "trigger"],
      [{ triggerSelected: true, infoPanelOpen: false, hasEditTab: true }, "trigger"],
      [{ triggerSelected: true, infoPanelOpen: true, hasEditTab: false }, "info"],
      [{ triggerSelected: true, infoPanelOpen: true, hasEditTab: true }, "info"],
    ];
    for (const [input, want] of expected) {
      expect(rightPaneOwner(input)).toBe(want);
    }
  });
});
