import { describe, it, expect, afterEach } from "vitest";
import { cssColor } from "./cssColor";

afterEach(() => {
  document.documentElement.style.cssText = "";
});

describe("cssColor — resolving a palette token at runtime (#759)", () => {
  it("reads the token's current value off the root element", () => {
    document.documentElement.style.setProperty("--color-acc", "#03714f");
    expect(cssColor("--color-acc", "#000000")).toBe("#03714f");
  });

  it("follows the token when the theme redefines it", () => {
    document.documentElement.style.setProperty("--color-acc", "#10b981");
    expect(cssColor("--color-acc", "#000000")).toBe("#10b981");
    document.documentElement.style.setProperty("--color-acc", "#03714f");
    expect(cssColor("--color-acc", "#000000")).toBe("#03714f");
  });

  it("falls back when the token is not defined", () => {
    expect(cssColor("--color-does-not-exist", "#abcdef")).toBe("#abcdef");
  });
});
