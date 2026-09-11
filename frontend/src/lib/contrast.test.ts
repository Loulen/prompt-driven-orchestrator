import { describe, it, expect } from "vitest";
import { contrastRatio } from "./contrast";

describe("contrastRatio — WCAG 2.1 relative luminance (#759)", () => {
  it("gives 21:1 for black on white and 1:1 for a colour on itself", () => {
    expect(contrastRatio("#000000", "#ffffff")).toBeCloseTo(21, 2);
    expect(contrastRatio("#10b981", "#10b981")).toBeCloseTo(1, 5);
  });

  it("is symmetric — order of the pair does not matter", () => {
    expect(contrastRatio("#0f1115", "#e6e8eb")).toBeCloseTo(
      contrastRatio("#e6e8eb", "#0f1115"),
      10,
    );
  });

  it("matches the published ratio for the classic AA grey on white", () => {
    // #767676 on white is the canonical 4.54:1 example — just over AA for body text.
    expect(contrastRatio("#767676", "#ffffff")).toBeCloseTo(4.54, 2);
  });

  it("accepts the three-digit shorthand", () => {
    expect(contrastRatio("#fff", "#000")).toBeCloseTo(21, 2);
  });

  it("rejects anything that is not a hex colour", () => {
    expect(() => contrastRatio("rebeccapurple", "#fff")).toThrow(/hex/i);
  });
});
