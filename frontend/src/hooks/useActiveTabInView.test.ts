import { describe, expect, it } from "vitest";
import { revealScroll } from "./useActiveTabInView";

describe("revealScroll", () => {
  it("leaves a strip alone when the item already shows", () => {
    expect(revealScroll(100, 400, 150, 120)).toBe(100);
  });

  it("scrolls back to an item before the view", () => {
    expect(revealScroll(300, 400, 120, 120)).toBe(120);
  });

  it("scrolls just enough to show an item past the view", () => {
    // Item from 900 to 1020, view of 400: the view ends at the item's end.
    expect(revealScroll(0, 400, 900, 120)).toBe(620);
  });

  it("shows the start of an item wider than the strip", () => {
    expect(revealScroll(0, 100, 500, 160)).toBe(500);
  });
});
