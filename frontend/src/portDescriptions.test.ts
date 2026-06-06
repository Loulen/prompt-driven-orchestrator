import { describe, it, expect } from "vitest";
import { getPortDescription } from "./portDescriptions";

describe("getPortDescription", () => {
  it("returns hardcoded description for merge output:merged", () => {
    expect(getPortDescription("merge", "output", "merged")).toBe(
      "Result artifact after merge",
    );
  });

  it("returns hardcoded description for merge input:branches", () => {
    expect(getPortDescription("merge", "input", "branches")).toBe(
      "Accumulates all incoming branches",
    );
  });

  it("returns yaml description when no hardcoded match", () => {
    expect(
      getPortDescription("doc-only", "input", "plan", "The implementation plan"),
    ).toBe("The implementation plan");
  });

  it("returns port name as fallback", () => {
    expect(getPortDescription("doc-only", "output", "review")).toBe("review");
  });

  it("prefers hardcoded over yaml description", () => {
    // ForEach is retired (#151); merge is the remaining first-class node whose
    // hardcoded port description wins over a YAML one.
    expect(
      getPortDescription("merge", "input", "branches", "custom desc"),
    ).toBe("Accumulates all incoming branches");
  });
});
