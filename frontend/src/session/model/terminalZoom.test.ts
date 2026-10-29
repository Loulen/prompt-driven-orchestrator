import { describe, expect, it } from "vitest";
import { clampTerminalZoom, stepTerminalZoom, terminalFontFactor } from "./terminalZoom";

describe("the Zoom du terminal (CORE-124)", () => {
  it("stays between 50 and 200 %", () => {
    expect(clampTerminalZoom(10)).toBe(50);
    expect(clampTerminalZoom(500)).toBe(200);
    expect(clampTerminalZoom(112.6)).toBe(113);
    expect(clampTerminalZoom(Number.NaN)).toBe(100);
  });

  it("multiplies with the zoom the host imposes", () => {
    expect(terminalFontFactor(100)).toBe(1);
    expect(terminalFontFactor(150, 0.5)).toBe(0.75);
    expect(terminalFontFactor(200, 2)).toBe(4);
    expect(terminalFontFactor(150, -1)).toBe(1.5);
    expect(terminalFontFactor(150, Number.NaN)).toBe(1.5);
  });

  it("steps to the next preset, and stays at either end", () => {
    expect(stepTerminalZoom(100, 1)).toBe(125);
    expect(stepTerminalZoom(100, -1)).toBe(75);
    expect(stepTerminalZoom(110, 1)).toBe(125);
    expect(stepTerminalZoom(110, -1)).toBe(100);
    expect(stepTerminalZoom(200, 1)).toBe(200);
    expect(stepTerminalZoom(50, -1)).toBe(50);
  });
});
