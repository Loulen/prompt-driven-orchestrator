import { describe, it, expect } from "vitest";
import {
  infoPanelButtons,
  resolveInfoTab,
  shouldCloseInfoOnTabChange,
  toggleAssistantTab,
  toggleInfoTab,
} from "./infoPanelReconcile";
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

  // #948: a Save that renames the pipeline rekeys the SAME tab (#774) — the
  // overlay still describes the tab in focus, so it stays open.
  it("stays open when the tab id change is the store's rekey of the same tab", () => {
    const rekey = { from: "demo", to: "renamed" };
    expect(shouldCloseInfoOnTabChange({ prevTabId: "demo", nextTabId: "renamed", infoOpen: true, rekey })).toBe(false);
  });

  it("still closes on a real tab switch after an earlier rekey", () => {
    const rekey = { from: "demo", to: "renamed" };
    expect(shouldCloseInfoOnTabChange({ prevTabId: "renamed", nextTabId: "other", infoOpen: true, rekey })).toBe(true);
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

// #938 (story PDO-3): the "agent" glyph and `(i)` are mutually exclusive and
// follow the tab the panel SHOWS.
describe("toolbar panel buttons (#938)", () => {
  const template = { hasRun: false, hasAssistant: true };
  const run = { hasRun: true, hasAssistant: false };

  it("resolves a tab the context lacks to Info", () => {
    expect(resolveInfoTab("assistant", run)).toBe("info");
    expect(resolveInfoTab("manager", template)).toBe("info");
    expect(resolveInfoTab("diff", template)).toBe("info");
    expect(resolveInfoTab("repositories", template)).toBe("info");
    expect(resolveInfoTab("yaml", template)).toBe("yaml");
    expect(resolveInfoTab("assistant", template)).toBe("assistant");
    expect(resolveInfoTab("manager", run)).toBe("manager");
  });

  it.each([
    ["closed", { open: false, tab: "assistant" as const }, false, false],
    ["on Assistant", { open: true, tab: "assistant" as const }, true, false],
    ["on Info", { open: true, tab: "info" as const }, false, true],
    ["on YAML", { open: true, tab: "yaml" as const }, false, true],
  ])("lights at most one button (%s)", (_label, state, assistantActive, infoActive) => {
    expect(infoPanelButtons(state, template)).toEqual({ assistantActive, infoActive });
  });

  it.each([
    // [panel state, glyph click, (i) click]
    [{ open: false, tab: "info" as const }, { open: true, tab: "assistant" }, { open: true, tab: "info" }],
    [{ open: true, tab: "assistant" as const }, { open: false, tab: "assistant" }, { open: true, tab: "info" }],
    [{ open: true, tab: "info" as const }, { open: true, tab: "assistant" }, { open: false, tab: "info" }],
    [{ open: true, tab: "yaml" as const }, { open: true, tab: "assistant" }, { open: false, tab: "yaml" }],
  ])("follows the transition table from %o", (state, afterGlyph, afterInfo) => {
    expect(toggleAssistantTab(state, template)).toEqual(afterGlyph);
    expect(toggleInfoTab(state, template)).toEqual(afterInfo);
  });

  it("keeps (i) a plain open/close toggle on a Run", () => {
    const closed = { open: false, tab: "info" as const };
    const opened = toggleInfoTab(closed, run);
    expect(opened).toEqual({ open: true, tab: "info" });
    expect(infoPanelButtons(opened, run)).toEqual({ assistantActive: false, infoActive: true });
    expect(toggleInfoTab({ open: true, tab: "manager" }, run)).toEqual({ open: false, tab: "manager" });
  });
});
