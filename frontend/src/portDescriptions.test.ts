import { describe, it, expect } from "vitest";
import { getPortDescription } from "./portDescriptions";

describe("getPortDescription", () => {
  it("returns hardcoded description for loop input:in", () => {
    expect(getPortDescription("loop", "input", "in")).toBe("Starts the loop");
  });

  it("returns hardcoded description for foreach output:body", () => {
    expect(getPortDescription("for-each", "output", "body")).toBe(
      "Fires once per item, in parallel",
    );
  });

  it("returns hardcoded description for switch input:in", () => {
    expect(getPortDescription("switch", "input", "in")).toBe(
      "Artifact to inspect for routing",
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
    expect(
      getPortDescription("loop", "input", "in", "custom desc"),
    ).toBe("Starts the loop");
  });
});
