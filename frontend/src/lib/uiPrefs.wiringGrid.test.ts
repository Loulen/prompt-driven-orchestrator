import { describe, it, expect, beforeEach } from "vitest";
import { loadWiringGridFeedback, saveWiringGridFeedback } from "./uiPrefs";

beforeEach(() => localStorage.clear());

describe("wiring grid feedback (#844)", () => {
  it("defaults to dots when nothing has been chosen", () => {
    expect(loadWiringGridFeedback()).toBe("dots");
  });

  it("round-trips each of the three choices", () => {
    for (const v of ["none", "dots", "lines"] as const) {
      saveWiringGridFeedback(v);
      expect(loadWiringGridFeedback()).toBe(v);
    }
  });

  it("falls back to dots on a value written by an older or broken client", () => {
    localStorage.setItem("pdo.ui.wiringGridFeedback", '"sparkles"');
    expect(loadWiringGridFeedback()).toBe("dots");
    localStorage.setItem("pdo.ui.wiringGridFeedback", "not json");
    expect(loadWiringGridFeedback()).toBe("dots");
  });
});
