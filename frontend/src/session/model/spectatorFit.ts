// #867 / ADR-0075 — a spectator renders the pilot's whole screen.
//
// Its grid is the pilot's cols × rows, whatever its own area: that is what
// removes tmux's cropping and dot padding. To make that grid fit, the font
// shrinks, down to a readability floor; below the floor the area scrolls. The
// font never grows past the normal size, even when the spectator's area is the
// bigger one.

import type { Dimensions } from "./altBufferResize";

/** The terminal's normal font size (px). */
export const BASE_FONT_SIZE = 11;
/** Smallest font a spectator is shown at; below it, the area scrolls instead. */
export const MIN_SPECTATOR_FONT_SIZE = 6;
const STEP = 0.5;

/**
 * The largest font size, between the floor and `base`, at which the `pilot` grid
 * fits the area. `measure(fontSize)` answers how many cells fit at that size
 * (xterm's FitAddon `proposeDimensions` after setting the font); `undefined`
 * means the area is not measurable yet, and the normal size is kept.
 */
export function spectatorFontSize(
  pilot: Dimensions,
  measure: (fontSize: number) => Dimensions | undefined,
  base: number = BASE_FONT_SIZE,
  floor: number = MIN_SPECTATOR_FONT_SIZE,
): number {
  const fits = (d: Dimensions | undefined) =>
    d !== undefined && d.cols >= pilot.cols && d.rows >= pilot.rows;

  const atBase = measure(base);
  if (atBase === undefined || atBase.cols <= 0 || atBase.rows <= 0) return base;
  if (fits(atBase)) return base;

  // Cells scale roughly with the font: start from the proportional guess, then
  // step down until the grid really fits (cell sizes are rounded to pixels).
  const ratio = Math.min(atBase.cols / pilot.cols, atBase.rows / pilot.rows);
  let size = Math.max(floor, Math.floor((base * ratio) / STEP) * STEP);
  while (size > floor && !fits(measure(size))) size -= STEP;
  return Math.max(size, floor);
}
