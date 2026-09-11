import { describe, it, expect } from "vitest";
import { terminalTheme, TERMINAL_INK } from "./terminalTheme";
import { contrastRatio, AA_TEXT } from "./contrast";

describe("terminalTheme (#759)", () => {
  it("returns the dark terminal byte for byte as it shipped", () => {
    // #759 adds a light terminal; it must not restyle the dark one. Any change
    // to these values is a deliberate decision, not a side effect.
    expect(terminalTheme("dark")).toEqual({
      background: "#0f1115",
      foreground: "#e6e8eb",
      cursor: "#10b981",
      selectionBackground: "#2a2d35",
      black: "#0f1115",
      red: "#ef4444",
      green: "#10b981",
      yellow: "#f59e0b",
      blue: "#3b82f6",
      magenta: "#8b5cf6",
      cyan: "#06b6d4",
      white: "#e6e8eb",
      brightBlack: "#5a6270",
      brightRed: "#f87171",
      brightGreen: "#34d399",
      brightYellow: "#fbbf24",
      brightBlue: "#60a5fa",
      brightMagenta: "#a78bfa",
      brightCyan: "#22d3ee",
      brightWhite: "#f8fafc",
    });
  });

  it("paints dark ink on a light ground for the light theme", () => {
    const light = terminalTheme("light");
    expect(contrastRatio(light.background, "#ffffff")).toBeLessThan(1.3);
    expect(contrastRatio(light.foreground, light.background)).toBeGreaterThanOrEqual(AA_TEXT);
  });

  it("keeps every ink colour at AA on the light ground", () => {
    const light = terminalTheme("light");
    const unreadable = TERMINAL_INK.filter(
      (slot) => contrastRatio(light[slot], light.background) < AA_TEXT,
    );
    expect(unreadable).toEqual([]);
  });

  it("fills every slot in both themes, so xterm never falls back to its own palette", () => {
    const slots = Object.keys(terminalTheme("dark"));
    expect(Object.keys(terminalTheme("light")).sort()).toEqual(slots.sort());
  });
});
