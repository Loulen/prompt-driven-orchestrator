/**
 * Where a new node lands (#825, FP finding).
 *
 * The rule under test is « two cards never overlap », and the reason it matters
 * is the edge between them: *First pipeline* asks the reader to click the edge
 * joining the implementer and the tester, and cards dropped 40 pixels apart bury
 * it completely.
 */
import { describe, expect, it } from "vitest";
import { CARD_HEIGHT, CARD_WIDTH, fallbackNodeSpot, freeDropSpot, type Point } from "./nodePlacement";

/** The check the canvas cares about: do the two card boxes intersect at all? */
function overlaps(a: Point, b: Point): boolean {
  return (
    Math.abs(a.x - b.x) < CARD_WIDTH && Math.abs(a.y - b.y) < CARD_HEIGHT
  );
}

const BASE: Point = { x: 400, y: 300 };

describe("dropping a node on the canvas", () => {
  it("leaves it where it was asked for when nothing is in the way", () => {
    expect(freeDropSpot(BASE, [])).toEqual(BASE);
    expect(freeDropSpot(BASE, [{ x: 4_000, y: 4_000 }])).toEqual(BASE);
  });

  it("clears a card sitting on the drop point, by a whole card", () => {
    const spot = freeDropSpot(BASE, [BASE]);
    expect(overlaps(spot, BASE)).toBe(false);
    // Right first: a pipeline reads left to right.
    expect(spot.x).toBeGreaterThan(BASE.x);
    expect(spot.y).toBe(BASE.y);
  });

  /**
   * The case that broke the tour: the reader creates two agents in a row, on a
   * fresh pipeline whose Start and End carry no position of their own — the
   * canvas lays them out, and they are on screen all the same. Every card must
   * end up clear of every other, or the edges between them cannot be clicked.
   */
  it("keeps three nodes created in a row from overlapping each other", () => {
    const placed: Point[] = [fallbackNodeSpot(0), fallbackNodeSpot(1)]; // Start, End
    for (let i = 0; i < 3; i++) placed.push(freeDropSpot(BASE, [...placed]));

    for (let i = 0; i < placed.length; i++) {
      for (let j = i + 1; j < placed.length; j++) {
        expect(overlaps(placed[i], placed[j]), `${i} over ${j}`).toBe(false);
      }
    }
  });

  /** The search walks a grid rather than sliding diagonally into the distance:
   *  a fourth card starts a new row instead of being a screen away. */
  it("wraps to a new row instead of running off the canvas", () => {
    const row: Point[] = [];
    for (let i = 0; i < 6; i++) row.push(freeDropSpot(BASE, [...row]));

    const spread = Math.max(...row.map((p) => p.x)) - BASE.x;
    expect(spread).toBeLessThanOrEqual(3 * (CARD_WIDTH + 40));
    expect(row.some((p) => p.y > BASE.y)).toBe(true);
  });

  /** A fresh pipeline writes Start and End without a `view`; the canvas gives
   *  them a column, and the drop search has to see that column. */
  it("clears a node the canvas positioned rather than the user", () => {
    const end = fallbackNodeSpot(1);
    const spot = freeDropSpot(end, [fallbackNodeSpot(0), end]);
    expect(overlaps(spot, end)).toBe(false);
    expect(overlaps(spot, fallbackNodeSpot(0))).toBe(false);
  });

  /** A canvas with no free spot left still gets an answer: searching forever is
   *  not an option, and landing somewhere plausible beats not landing. */
  it("gives up gracefully on a canvas packed with cards", () => {
    const packed: Point[] = [];
    for (let row = 0; row < 8; row++) {
      for (let col = 0; col < 8; col++) {
        packed.push({ x: BASE.x + col * 40, y: BASE.y + row * 40 });
      }
    }
    const spot = freeDropSpot(BASE, packed);
    expect(Number.isFinite(spot.x) && Number.isFinite(spot.y)).toBe(true);
  });
});
