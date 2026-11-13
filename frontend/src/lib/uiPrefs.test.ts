import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  loadChildRunsExpanded,
  loadCollapsedRunGroups,
  loadNodeDurationMode,
  loadTabsDisabled,
  loadTranscriptOrder,
  saveChildRunsExpanded,
  saveCollapsedRunGroups,
  saveNodeDurationMode,
  saveTabsDisabled,
  saveTranscriptOrder,
} from "./uiPrefs";

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

describe("uiPrefs — childRunsExpanded (#783)", () => {
  it("defaults to TRUE when absent — « déplié par défaut »", () => {
    expect(loadChildRunsExpanded()).toBe(true);
  });

  it("round-trips under the pdo.ui.childRunsExpanded key", () => {
    saveChildRunsExpanded(false);
    expect(localStorage.getItem("pdo.ui.childRunsExpanded")).toBe("false");
    expect(loadChildRunsExpanded()).toBe(false);
    saveChildRunsExpanded(true);
    expect(loadChildRunsExpanded()).toBe(true);
  });

  it("falls back to true for garbage or a non-boolean", () => {
    localStorage.setItem("pdo.ui.childRunsExpanded", "nope{");
    expect(loadChildRunsExpanded()).toBe(true);
    localStorage.setItem("pdo.ui.childRunsExpanded", JSON.stringify("false"));
    expect(loadChildRunsExpanded()).toBe(true);
  });
});

describe("uiPrefs — collapsedRunGroups", () => {
  it("defaults to no folded group", () => {
    expect(loadCollapsedRunGroups()).toEqual([]);
  });

  it("round-trips under the pdo.ui.collapsedRunGroups key", () => {
    saveCollapsedRunGroups(["project:p1", "/code/api"]);
    expect(localStorage.getItem("pdo.ui.collapsedRunGroups")).toBe('["project:p1","/code/api"]');
    expect(loadCollapsedRunGroups()).toEqual(["project:p1", "/code/api"]);
  });

  it("drops garbage and what is not a key", () => {
    localStorage.setItem("pdo.ui.collapsedRunGroups", "nope{");
    expect(loadCollapsedRunGroups()).toEqual([]);
    localStorage.setItem("pdo.ui.collapsedRunGroups", JSON.stringify(["project:p1", 3]));
    expect(loadCollapsedRunGroups()).toEqual(["project:p1"]);
  });
});

describe("uiPrefs — nodeDurationMode", () => {
  it("defaults to total when the key is absent", () => {
    expect(loadNodeDurationMode()).toBe("total");
  });

  it("round-trips active/total under pdo.ui.nodeDurationMode", () => {
    saveNodeDurationMode("active");
    expect(localStorage.getItem("pdo.ui.nodeDurationMode")).toBe('"active"');
    expect(loadNodeDurationMode()).toBe("active");
    saveNodeDurationMode("total");
    expect(loadNodeDurationMode()).toBe("total");
  });

  it("falls back to total on garbage or an unknown mode", () => {
    localStorage.setItem("pdo.ui.nodeDurationMode", "not-json{");
    expect(loadNodeDurationMode()).toBe("total");
    localStorage.setItem("pdo.ui.nodeDurationMode", JSON.stringify("waiting"));
    expect(loadNodeDurationMode()).toBe("total");
  });
});

describe("uiPrefs — transcript order", () => {
  it("defaults to newest first", () => {
    expect(loadTranscriptOrder()).toBe("newest-first");
  });

  it("round-trips the order under pdo.ui.transcriptOrder", () => {
    saveTranscriptOrder("oldest-first");
    expect(localStorage.getItem("pdo.ui.transcriptOrder")).toBe('"oldest-first"');
    expect(loadTranscriptOrder()).toBe("oldest-first");
    saveTranscriptOrder("newest-first");
    expect(loadTranscriptOrder()).toBe("newest-first");
  });

  it("falls back to newest first on garbage or an unknown order", () => {
    localStorage.setItem("pdo.ui.transcriptOrder", "not-json{");
    expect(loadTranscriptOrder()).toBe("newest-first");
    localStorage.setItem("pdo.ui.transcriptOrder", JSON.stringify("sideways"));
    expect(loadTranscriptOrder()).toBe("newest-first");
  });

  it("degrades to the default when the storage is unavailable", () => {
    localStorage.setItem("pdo.ui.transcriptOrder", JSON.stringify("oldest-first"));
    // On the prototype, as the other storage tests: jsdom's Storage takes an
    // own property set on it for an item, so a spy on the instance never runs.
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("SecurityError");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("QuotaExceededError");
    });
    expect(() => saveTranscriptOrder("oldest-first")).not.toThrow();
    expect(loadTranscriptOrder()).toBe("newest-first");
  });
});
