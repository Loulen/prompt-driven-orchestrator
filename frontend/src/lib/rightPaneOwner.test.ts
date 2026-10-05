import { describe, it, expect } from "vitest";
import { rightPaneCollapsed, rightPaneOwner } from "./rightPaneOwner";
import type { RightPaneContentInputs, RightPaneInputs } from "./rightPaneOwner";

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

// Notion #6 / #949 — « Panneau de droite replié »: the pane collapses exactly
// when the branch App renders for its owner would paint nothing.
describe("rightPaneCollapsed", () => {
  const tpl: RightPaneContentInputs = {
    owner: "editTab",
    selectionKind: "none",
    runPanelShown: false,
    legacyHasContent: false,
  };

  it("collapses on a template tab with an empty selection (info closed)", () => {
    expect(rightPaneCollapsed(tpl)).toBe(true);
  });

  it("collapses on the home screen (no tab, nothing selected)", () => {
    expect(rightPaneCollapsed({ ...tpl, owner: "selectedNode" })).toBe(true);
  });

  it("stays open while Pipeline info is open, even with nothing selected", () => {
    expect(rightPaneCollapsed({ ...tpl, owner: "info" })).toBe(false);
  });

  it("stays open for a selected Trigger", () => {
    expect(rightPaneCollapsed({ ...tpl, owner: "trigger" })).toBe(false);
  });

  it.each(["node", "edge", "region", "note"] as const)(
    "stays open for a %s selection on a template",
    (selectionKind) => {
      expect(rightPaneCollapsed({ ...tpl, selectionKind })).toBe(false);
    },
  );

  it("keeps the Run panel on a Run tab with an empty selection (Q6)", () => {
    expect(rightPaneCollapsed({ ...tpl, runPanelShown: true })).toBe(false);
    expect(
      rightPaneCollapsed({ ...tpl, selectionKind: "run", runPanelShown: true }),
    ).toBe(false);
  });

  it("collapses a Run tab whose state is not loaded yet (nothing painted)", () => {
    expect(rightPaneCollapsed({ ...tpl, selectionKind: "run" })).toBe(true);
  });

  it("keeps the no-tab path open when it has content (run node, Run archived — Q7)", () => {
    expect(
      rightPaneCollapsed({ ...tpl, owner: "selectedNode", legacyHasContent: true }),
    ).toBe(false);
  });
});
