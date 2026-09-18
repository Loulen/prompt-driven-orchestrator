import { describe, it, expect, beforeEach } from "vitest";
import {
  ALL_NODE_KINDS,
  loadCompletedOnly,
  loadExcludeUserWait,
  loadNodeKinds,
  performanceFiltersDeviate,
  saveCompletedOnly,
  saveExcludeUserWait,
  saveNodeKinds,
} from "./statsPrefs";

describe("statsPrefs (#810)", () => {
  beforeEach(() => localStorage.clear());

  it("opens a fresh browser on the design's defaults", () => {
    expect(loadCompletedOnly()).toBe(true);
    expect(loadExcludeUserWait()).toBe(true);
    expect(loadNodeKinds()).toEqual([...ALL_NODE_KINDS]);
    expect(performanceFiltersDeviate(true, [...ALL_NODE_KINDS])).toBe(false);
  });

  it("round-trips each setting under its own pdo.stats key", () => {
    saveCompletedOnly(false);
    saveExcludeUserWait(false);
    saveNodeKinds(["standard", "interactive"]);

    expect(localStorage.getItem("pdo.stats.completed_only")).toBe("false");
    expect(localStorage.getItem("pdo.stats.exclude_user_wait")).toBe("false");
    // Stored in the canonical order, whatever order the caller passed.
    expect(localStorage.getItem("pdo.stats.node_kinds")).toBe(
      '["interactive","standard"]',
    );

    expect(loadCompletedOnly()).toBe(false);
    expect(loadExcludeUserWait()).toBe(false);
    expect(loadNodeKinds()).toEqual(["interactive", "standard"]);
    expect(performanceFiltersDeviate(false, ["interactive", "standard"])).toBe(
      true,
    );
  });

  it("keeps an empty selection but repairs a corrupt one", () => {
    // Unchecking every chip is a choice, and survives a reload.
    saveNodeKinds([]);
    expect(loadNodeKinds()).toEqual([]);

    // A foreign or unparseable entry is not: never render an empty tab for a
    // reason the user cannot see.
    localStorage.setItem("pdo.stats.node_kinds", '["nonsense"]');
    expect(loadNodeKinds()).toEqual([...ALL_NODE_KINDS]);
    localStorage.setItem("pdo.stats.node_kinds", "{oops");
    expect(loadNodeKinds()).toEqual([...ALL_NODE_KINDS]);
    localStorage.setItem("pdo.stats.completed_only", "not json");
    expect(loadCompletedOnly()).toBe(true);
  });
});
