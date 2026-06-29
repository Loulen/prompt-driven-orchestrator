import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { loadDismissed, saveDismissed } from "./dismissedBanners";

beforeEach(() => {
  localStorage.clear();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("dismissedBanners (#268)", () => {
  it("round-trips a set of ids through save/load", () => {
    saveDismissed("p1", new Set(["fanout:worker", "fanout:fixer"]));
    expect(loadDismissed("p1")).toEqual(new Set(["fanout:worker", "fanout:fixer"]));
  });

  it("persists per-pipeline under a namespaced key", () => {
    saveDismissed("p1", new Set(["fanout:a"]));
    expect(localStorage.getItem("pdo.banner.dismissed.p1")).toBe(
      JSON.stringify(["fanout:a"]),
    );
    // A different pipeline tab is an independent set.
    expect(loadDismissed("p2")).toEqual(new Set());
  });

  it("returns an empty set when no key is present", () => {
    expect(loadDismissed("never-seen")).toEqual(new Set());
  });

  it("returns an empty set for non-JSON garbage", () => {
    localStorage.setItem("pdo.banner.dismissed.p1", "not-json{");
    expect(loadDismissed("p1")).toEqual(new Set());
  });

  it("returns an empty set when the stored value is not an array", () => {
    localStorage.setItem("pdo.banner.dismissed.p1", JSON.stringify({ a: true }));
    expect(loadDismissed("p1")).toEqual(new Set());
  });

  it("returns an empty set when the array holds non-strings", () => {
    localStorage.setItem("pdo.banner.dismissed.p1", JSON.stringify(["ok", 7]));
    expect(loadDismissed("p1")).toEqual(new Set());
  });

  it("degrades gracefully when localStorage.getItem throws (private mode)", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("SecurityError");
    });
    expect(loadDismissed("p1")).toEqual(new Set());
  });

  it("swallows a throwing setItem (quota / disabled) without raising", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("QuotaExceededError");
    });
    expect(() => saveDismissed("p1", new Set(["fanout:a"]))).not.toThrow();
  });
});
