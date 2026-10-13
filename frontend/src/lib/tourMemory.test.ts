/**
 * What the browser remembers about tours (#823) and the welcome-modal rule
 * (spec #821, stories 1–4 and 6).
 */
import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  drawTourIdentity,
  loadTourOffered,
  loadToursDone,
  markTourDone,
  markTourOffered,
  resetTourMemory,
  shouldOfferWelcome,
  tourIdentity,
  TOUR_IDENTITY_FORMAT,
  withTourIdentity,
} from "./tourMemory";

beforeEach(() => localStorage.clear());

describe("the welcome rule", () => {
  it("offers a tour to a browser never asked, on an instance with no Run", () => {
    expect(shouldOfferWelcome({ offered: false, runsLoaded: true, runCount: 0 })).toBe(true);
  });

  it("never asks twice — any answer sets the key (story 3)", () => {
    expect(shouldOfferWelcome({ offered: true, runsLoaded: true, runCount: 0 })).toBe(false);
  });

  it("does not take a seasoned user on a new browser for a beginner (story 4)", () => {
    expect(shouldOfferWelcome({ offered: false, runsLoaded: true, runCount: 3 })).toBe(false);
  });

  it("stays quiet until the run list has actually answered", () => {
    // Before `GET /runs` returns, "no Runs" and "not loaded yet" look identical —
    // without this guard the modal would flash on every load of every instance.
    expect(shouldOfferWelcome({ offered: false, runsLoaded: false, runCount: 0 })).toBe(false);
  });
});

describe("the keys", () => {
  it("remembers that the welcome was answered", () => {
    expect(loadTourOffered()).toBe(false);
    markTourOffered();
    expect(loadTourOffered()).toBe(true);
  });

  it("remembers a finished tour with its date, and keeps the others", () => {
    markTourDone("first-pipeline", new Date("2026-09-20T10:00:00Z"));
    markTourDone("first-run", new Date("2026-09-21T10:00:00Z"));
    expect(Object.keys(loadToursDone()).sort()).toEqual(["first-pipeline", "first-run"]);
    expect(loadToursDone()["first-pipeline"]).toBe("2026-09-20T10:00:00.000Z");
  });

  it("keeps one key per tour, so one bad entry costs only its own checkmark", () => {
    markTourDone("first-pipeline", new Date("2026-09-20T10:00:00Z"));
    localStorage.setItem("pdo.tour.done.first-run", "");
    expect(loadToursDone()).toEqual({ "first-pipeline": "2026-09-20T10:00:00.000Z" });
    // And nothing else in the `pdo.*` namespace is mistaken for a checkmark.
    localStorage.setItem("pdo.ui.tabsDisabled", "true");
    expect(Object.keys(loadToursDone())).toEqual(["first-pipeline"]);
  });

  it("forgets both the checkmarks and the welcome prompt on reset", () => {
    markTourOffered();
    markTourDone("first-pipeline");
    resetTourMemory();
    expect(loadTourOffered()).toBe(false);
    expect(loadToursDone()).toEqual({});
    // And the modal is proposable again on a Run-less instance.
    expect(shouldOfferWelcome({ offered: loadTourOffered(), runsLoaded: true, runCount: 0 })).toBe(
      true,
    );
  });
});

describe("the identité de tour", () => {
  it("is drawn once, as four base36 characters, and kept by the browser", () => {
    const id = tourIdentity();
    expect(id).toMatch(/^[0-9a-z]{4}$/);
    expect(localStorage.getItem("pdo.tour.identity")).toBe(id);
    // Stable: a later read (a reload reads the same store) gives the same identity.
    expect(tourIdentity()).toBe(id);
    expect(tourIdentity()).toBe(id);
  });

  it("is read back from the store — another browser's store gives another identity", () => {
    localStorage.setItem("pdo.tour.identity", "aaaa");
    expect(tourIdentity()).toBe("aaaa");
    localStorage.setItem("pdo.tour.identity", "bbbb");
    expect(tourIdentity()).toBe("bbbb");
  });

  it("redraws a stored value that is not an identity", () => {
    localStorage.setItem("pdo.tour.identity", "NOT-AN-ID");
    const id = tourIdentity();
    expect(id).toMatch(TOUR_IDENTITY_FORMAT);
    expect(localStorage.getItem("pdo.tour.identity")).toBe(id);
  });

  it("keeps one identity for the session when the store refuses to keep it", () => {
    const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("quota");
    });
    try {
      const first = tourIdentity();
      expect(first).toMatch(TOUR_IDENTITY_FORMAT);
      expect(tourIdentity()).toBe(first);
    } finally {
      setItem.mockRestore();
    }
  });

  it("draws identities that differ from one browser to the next", () => {
    const drawn = new Set(Array.from({ length: 50 }, () => drawTourIdentity()));
    for (const id of drawn) expect(id).toMatch(TOUR_IDENTITY_FORMAT);
    // 36^4 ≈ 1.7 M identities: fifty draws colliding into a handful would mean no randomness.
    expect(drawn.size).toBeGreaterThan(45);
  });

  it("suffixes a name with this browser's identity", () => {
    localStorage.setItem("pdo.tour.identity", "k3x9");
    expect(withTourIdentity("tour-overview")).toBe("tour-overview-k3x9");
    expect(withTourIdentity("tour-overview", "p7qa")).toBe("tour-overview-p7qa");
  });

  it("survives a reset of the tutorial memory, which would orphan the tours' objects", () => {
    localStorage.setItem("pdo.tour.identity", "k3x9");
    markTourOffered();
    markTourDone("overview");
    resetTourMemory();
    expect(tourIdentity()).toBe("k3x9");
  });
});
