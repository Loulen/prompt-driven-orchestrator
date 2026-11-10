import { describe, expect, it } from "vitest";
import { newNodeName } from "./newNodeName";

describe("newNodeName", () => {
  it("is the base name on a canvas that does not use it", () => {
    expect(newNodeName("implementer", ["start", "end"])).toBe("implementer");
  });

  // FP of CORE-227 (C3): every card « + › Node » made was called implementer.
  it("numbers the base name past the ones already taken", () => {
    expect(newNodeName("implementer", ["implementer"])).toBe("implementer-2");
    expect(newNodeName("implementer", ["implementer", "implementer-2"])).toBe("implementer-3");
    expect(newNodeName("script", ["script", "script-3"])).toBe("script-2");
  });
});
