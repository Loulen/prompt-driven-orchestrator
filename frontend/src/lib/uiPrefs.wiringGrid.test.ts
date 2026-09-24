import { describe, it, expect, beforeEach } from "vitest";
import {
  loadWiringGridFeedback,
  loadWiringGridSize,
  saveWiringGridFeedback,
  saveWiringGridSize,
} from "./uiPrefs";

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

describe("wiring grid size (#877)", () => {
  it("defaults to M when nothing has been chosen", () => {
    expect(loadWiringGridSize()).toBe("M");
  });

  it("round-trips each of the three sizes", () => {
    for (const v of ["S", "M", "L"] as const) {
      saveWiringGridSize(v);
      expect(loadWiringGridSize()).toBe(v);
    }
  });

  it("falls back to M on a value it does not know", () => {
    localStorage.setItem("pdo.ui.wiringGridSize", '"XL"');
    expect(loadWiringGridSize()).toBe("M");
    localStorage.setItem("pdo.ui.wiringGridSize", "not json");
    expect(loadWiringGridSize()).toBe("M");
  });
});
