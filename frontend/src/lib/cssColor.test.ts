import { describe, it, expect, afterEach } from "vitest";
import { cssColor } from "./cssColor";

afterEach(() => {
  document.documentElement.style.cssText = "";
});

describe("cssColor — resolving a token at runtime (#759, ADR-0091)", () => {
  it("reads the token's current value off the root element", () => {
    document.documentElement.style.setProperty("--primary", "#03714f");
    expect(cssColor("--primary")).toBe("#03714f");
  });

  it("follows the token when the theme redefines it", () => {
    document.documentElement.style.setProperty("--primary", "#10b981");
    expect(cssColor("--primary")).toBe("#10b981");
    document.documentElement.style.setProperty("--primary", "#03714f");
    expect(cssColor("--primary")).toBe("#03714f");
  });

  it("resolves an OKLCH palette value to sRGB", () => {
    document.documentElement.style.setProperty("--text", "oklch(100% 0 0)");
    expect(cssColor("--text")).toBe("#ffffff");
    document.documentElement.style.setProperty("--text", "oklch(0% 0 0)");
    expect(cssColor("--text")).toBe("#000000");
  });

  it("keeps the alpha of a translucent value", () => {
    document.documentElement.style.setProperty("--shadow", "oklch(0% 0 0 / 0.45)");
    expect(cssColor("--shadow")).toBe("rgba(0, 0, 0, 0.45)");
  });

  it("falls back when the token is not defined, to a neutral grey by default", () => {
    expect(cssColor("--does-not-exist", "#abcdef")).toBe("#abcdef");
    expect(cssColor("--does-not-exist")).toBe("rgb(128, 128, 128)");
  });
});
