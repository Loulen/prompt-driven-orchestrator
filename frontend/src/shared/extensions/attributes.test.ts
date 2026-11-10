import { describe, expect, it } from "vitest";
import { isAttributeFilterActive, matchesAttributes } from "./attributes";

describe("matchesAttributes", () => {
  it("keeps an entity carrying every attribute asked for", () => {
    const carried = { "test/started_by": "alice", "other/team": "red" };
    expect(matchesAttributes(carried, { "test/started_by": "alice" })).toBe(true);
    expect(matchesAttributes(carried, { "test/started_by": "bob" })).toBe(false);
    expect(matchesAttributes(undefined, { "test/started_by": "alice" })).toBe(false);
    expect(matchesAttributes(undefined, {})).toBe(true);
    expect(isAttributeFilterActive({})).toBe(false);
    expect(isAttributeFilterActive({ "test/started_by": "alice" })).toBe(true);
  });
});
