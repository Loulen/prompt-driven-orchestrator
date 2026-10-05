import { describe, expect, it } from "vitest";
import { runCreator } from "./runCreator";

describe("runCreator", () => {
  it("reads the Run's creator, null when it has none", () => {
    const alice = { kind: "identified", namespace: "test", id: "alice" };
    expect(runCreator({ run_id: "r1", created_by: alice })).toEqual(alice);
    expect(runCreator({ run_id: "r1" })).toBeNull();
    expect(runCreator({ run_id: "r1", created_by: null })).toBeNull();
  });
});
