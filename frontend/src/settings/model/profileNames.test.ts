import { beforeEach, describe, expect, it, vi } from "vitest";
import { forgetSessionProfileNames, profileLabel, rememberProfileNames } from "./profileNames";

beforeEach(() => {
  localStorage.clear();
  forgetSessionProfileNames();
});

describe("the names of the agent profiles this browser has seen", () => {
  it("name a profile once it is deleted", () => {
    rememberProfileNames([{ id: "agp-1", name: "Reviewer fast" }]);
    // The profile is gone from the next list: its name stays.
    rememberProfileNames([]);
    expect(profileLabel("agp-1")).toBe("Reviewer fast");
  });

  it("follow a rename", () => {
    rememberProfileNames([{ id: "agp-1", name: "Reviewer" }]);
    rememberProfileNames([{ id: "agp-1", name: "Reviewer fast" }]);
    expect(profileLabel("agp-1")).toBe("Reviewer fast");
  });

  it("fall back on the id of a profile never seen", () => {
    expect(profileLabel("agp-9")).toBe("agp-9");
  });

  it("survive a reload", () => {
    rememberProfileNames([{ id: "agp-1", name: "Reviewer fast" }]);
    forgetSessionProfileNames();
    expect(profileLabel("agp-1")).toBe("Reviewer fast");
  });

  it("keep what another tab stored", () => {
    localStorage.setItem("pdo.agentProfiles.names", JSON.stringify({ "agp-2": "Planner" }));
    rememberProfileNames([{ id: "agp-1", name: "Reviewer fast" }]);
    expect(JSON.parse(localStorage.getItem("pdo.agentProfiles.names")!)).toEqual({
      "agp-1": "Reviewer fast",
      "agp-2": "Planner",
    });
  });

  it("say the name this session listed last, even when storage is full", () => {
    rememberProfileNames([{ id: "agp-1", name: "Reviewer" }]);
    const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("QuotaExceededError");
    });
    rememberProfileNames([{ id: "agp-1", name: "Reviewer fast" }, { id: "agp-3", name: "Writer" }]);
    rememberProfileNames([]);
    setItem.mockRestore();
    expect(profileLabel("agp-1")).toBe("Reviewer fast");
    expect(profileLabel("agp-3")).toBe("Writer");
  });

  it("read a corrupt store as nothing seen", () => {
    localStorage.setItem("pdo.agentProfiles.names", "[1, 2");
    expect(profileLabel("agp-1")).toBe("agp-1");
    rememberProfileNames([{ id: "agp-1", name: "Reviewer fast" }]);
    expect(profileLabel("agp-1")).toBe("Reviewer fast");
  });
});
