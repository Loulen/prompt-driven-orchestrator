import { describe, expect, it } from "vitest";
import { evaluateWhen, lookup } from "./when";

const context = {
  location: "topbar/end",
  run: { id: "r1", status: "running", attributes: { "test/started_by": "alice" }, name: "" },
  project: null,
  count: 0,
};

describe("a when clause", () => {
  it("reads dotted keys, an absent one as nothing", () => {
    expect(lookup(context, "run.attributes.test/started_by")).toBe("alice");
    expect(lookup(context, "project.name")).toBeUndefined();
    expect(lookup(context, "nope.deeper")).toBeUndefined();
  });

  it.each([
    [{ has: "run" }, true],
    [{ has: "project" }, false],
    [{ has: "run.name" }, false],
    [{ has: "count" }, true],
    [{ eq: ["run.status", "running"] }, true],
    [{ eq: ["run.status", "done"] }, false],
    [{ eq: ["count", "0"] }, true],
    [{ eq: ["project.id", "undefined"] }, false],
    [{ not: { has: "project" } }, true],
    [{ all: [{ has: "run" }, { eq: ["location", "topbar/end"] }] }, true],
    [{ all: [] }, true],
    [{ any: [{ has: "project" }, { eq: ["run.attributes.test/started_by", "alice"] }] }, true],
    [{ any: [] }, false],
  ] as const)("%j holds: %s", (when, expected) => {
    expect(evaluateWhen(when as never, context)).toBe(expected);
  });

  it("always holds when absent, never for an operator this front does not know", () => {
    expect(evaluateWhen(undefined, context)).toBe(true);
    expect(evaluateWhen({ matches: ["run.id", "r.*"] } as never, context)).toBe(false);
  });
});
