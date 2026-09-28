/**
 * Rectangle arithmetic for the tour hook (#823): what an element's box is,
 * the padded union the Projecteur's hole is cut from, and whether an element
 * is laid out at all. Reads the DOM, writes nothing.
 */

import type { TourRect } from "./tourView";

/** Breathing room around the hole, so the ring never clips the target's own border. */
export const HOLE_PADDING = 6;

/**
 * What a `soft` step's dashed zone is resolved from — the popup the option lives
 * in. The ARIA role is the general answer; `data-slot` is this codebase's own menu
 * (Base UI's `Menu.Popup` via `components/ui/dropdown-menu`), listed so the zone
 * never silently degrades to "block the whole menu but one option" if the
 * primitive's markup shifts.
 *
 * `[role="dialog"]` joined the list in #824: three of *First run*'s targets sit
 * inside one (the filesystem explorer, the Agent popover, the Skills popover), and
 * those are exactly the surfaces where the user must keep every other control —
 * scroll the tree, climb a folder, press "Select this folder". Nearest ancestor
 * wins, so the New Run modal underneath is never the zone.
 */
export const MENU_CONTAINERS =
  '[role="menu"], [role="listbox"], [role="dialog"], [data-slot="dropdown-menu-content"]';

export function rectOf(el: Element): TourRect {
  const r = el.getBoundingClientRect();
  return { top: r.top, left: r.left, width: r.width, height: r.height };
}

export function union(rects: TourRect[], padding: number): TourRect | null {
  if (rects.length === 0) return null;
  const top = Math.min(...rects.map((r) => r.top));
  const left = Math.min(...rects.map((r) => r.left));
  const bottom = Math.max(...rects.map((r) => r.top + r.height));
  const right = Math.max(...rects.map((r) => r.left + r.width));
  return {
    top: top - padding,
    left: left - padding,
    width: right - left + padding * 2,
    height: bottom - top + padding * 2,
  };
}

export function sameRect(a: TourRect | null, b: TourRect | null): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  return (
    Math.abs(a.top - b.top) < 0.5 &&
    Math.abs(a.left - b.left) < 0.5 &&
    Math.abs(a.width - b.width) < 0.5 &&
    Math.abs(a.height - b.height) < 0.5
  );
}

/** An element the user can actually aim at: in the document AND laid out. A zero-box
 *  element (a collapsed pane, a menu mid-animation) is "not there yet" for a tour. */
export function visible(el: Element | null): el is Element {
  if (!el) return false;
  const r = el.getBoundingClientRect();
  return r.width > 0 && r.height > 0;
}
