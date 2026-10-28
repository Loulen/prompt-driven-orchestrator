import { describe, it, expect } from "vitest";
import { terminalTheme, TERMINAL_INK } from "./terminalTheme";
import { contrastRatio, AA_TEXT } from "../../lib/contrast";

describe("terminalTheme (#759, an always-dark screen)", () => {
  it("is one dark palette, whatever the theme of the interface", () => {
    const theme = terminalTheme();
    expect(theme.background).toBe("#0b0b0d");
    expect(theme.black).toBe(theme.background);
    // #772: a selection the user can see.
    expect(theme.selectionBackground).toMatch(/^#3b82f6/);
  });

  it("reads its text, in the secondary ink, at AA on the dark ground", () => {
    const theme = terminalTheme();
    expect(contrastRatio(theme.foreground, theme.background)).toBeGreaterThanOrEqual(AA_TEXT);
    expect(contrastRatio(theme.white, theme.background)).toBeGreaterThanOrEqual(AA_TEXT);
  });

  it("fills every ink slot, so xterm never falls back to its own palette", () => {
    const theme = terminalTheme();
    expect(TERMINAL_INK.filter((slot) => !theme[slot])).toEqual([]);
  });
});
