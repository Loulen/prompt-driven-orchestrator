import { describe, it, expect } from "vitest";
import { formatColor, parseColor } from "./colorValue";

describe("parseColor — the colour forms of the palette and of a computed style", () => {
  it("reads hex, rgb() and rgba()", () => {
    expect(parseColor("#fff")).toEqual({ r: 1, g: 1, b: 1, a: 1 });
    expect(formatColor(parseColor("#14171d")!)).toBe("#14171d");
    expect(formatColor(parseColor("rgb(20, 23, 29)")!)).toBe("#14171d");
    expect(formatColor(parseColor("rgba(0, 0, 0, 0.5)")!)).toBe("rgba(0, 0, 0, 0.5)");
  });

  it("converts OKLCH, percent or number lightness, with or without alpha", () => {
    expect(formatColor(parseColor("oklch(100% 0 0)")!)).toBe("#ffffff");
    expect(formatColor(parseColor("oklch(0.5 0 0)")!)).toBe(formatColor(parseColor("oklch(50% 0 0)")!));
    // The ruby of the light theme: the brand writes it #911a46 in its favicon.
    expect(formatColor(parseColor("oklch(43.6% 0.155 5)")!)).toBe("#911a46");
    expect(parseColor("oklch(0% 0 0 / 0.45)")!.a).toBeCloseTo(0.45, 5);
  });

  it("reads color(srgb …), as a browser computes a color-mix()", () => {
    expect(formatColor(parseColor("color(srgb 1 0 0 / 0.5)")!)).toBe("rgba(255, 0, 0, 0.5)");
  });

  it("gives null for what it cannot read", () => {
    expect(parseColor("rebeccapurple")).toBeNull();
    expect(parseColor("var(--x)")).toBeNull();
    expect(parseColor("#12345")).toBeNull();
  });
});
