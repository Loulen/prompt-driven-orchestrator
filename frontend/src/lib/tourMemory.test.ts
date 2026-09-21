/**
 * What the browser remembers about tours (#823) and the welcome-modal rule
 * (spec #821, stories 1–4 and 6).
 */
import { describe, it, expect, beforeEach } from "vitest";
import {
  loadTourOffered,
  loadToursDone,
  markTourDone,
  markTourOffered,
  resetTourMemory,
  shouldOfferWelcome,
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
