import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { loadTabsDisabled, saveTabsDisabled } from "./uiPrefs";

beforeEach(() => {
  localStorage.clear();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("uiPrefs — tabsDisabled (#342)", () => {
  it("round-trips true/false through save/load", () => {
    saveTabsDisabled(true);
    expect(loadTabsDisabled()).toBe(true);
    saveTabsDisabled(false);
    expect(loadTabsDisabled()).toBe(false);
  });

  it("persists under the pdo.ui.tabsDisabled key", () => {
    saveTabsDisabled(true);
    expect(localStorage.getItem("pdo.ui.tabsDisabled")).toBe("true");
  });

  it("defaults to false when the key is absent (multi-tab default)", () => {
    expect(loadTabsDisabled()).toBe(false);
  });

  it("defaults to false for non-JSON garbage", () => {
    localStorage.setItem("pdo.ui.tabsDisabled", "not-json{");
    expect(loadTabsDisabled()).toBe(false);
  });

  it("defaults to false when the stored value is not a boolean", () => {
    localStorage.setItem("pdo.ui.tabsDisabled", JSON.stringify("true"));
    expect(loadTabsDisabled()).toBe(false);
    localStorage.setItem("pdo.ui.tabsDisabled", JSON.stringify(1));
    expect(loadTabsDisabled()).toBe(false);
  });

  it("degrades to false when localStorage.getItem throws (private mode)", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("SecurityError");
    });
    expect(loadTabsDisabled()).toBe(false);
  });

  it("swallows a throwing setItem (quota / disabled) without raising", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("QuotaExceededError");
    });
    expect(() => saveTabsDisabled(true)).not.toThrow();
  });
});
