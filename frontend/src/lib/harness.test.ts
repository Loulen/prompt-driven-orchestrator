import { describe, expect, it } from "vitest";
import { harnessColor } from "./harness";

describe("harnessColor (#638)", () => {
  it("pins Copilot blue and Claude orange", () => {
    expect(harnessColor("copilot")).toBe("#58a6ff");
    expect(harnessColor("claude")).toBe("#f0883e");
  });

  it("assigns a stable colour to a future harness", () => {
    expect(harnessColor("future-harness")).toBe(harnessColor("future-harness"));
    expect(harnessColor("future-harness")).not.toBe(harnessColor("another-harness"));
  });
});
