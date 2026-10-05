import { describe, expect, it } from "vitest";
import { runCreator } from "./runCreator";

describe("runCreator", () => {
  it("reads the Run's creator, null when it has none", () => {
    const alice = { kind: "identified" as const, namespace: "test", id: "alice" };
    expect(runCreator({ created_by: alice })).toEqual(alice);
    expect(runCreator({})).toBeNull();
  });
});
