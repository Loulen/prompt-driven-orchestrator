/**
 * Wheel relay for the Projecteur (#837, point 1).
 *
 * The four blockers of the spotlight exist to swallow *clicks*. They have no
 * scrollable ancestor, so a wheel gesture that lands on one dies there: Firefox
 * resolves the scroll target from the DOM element under the cursor and finds
 * nothing to move, which froze the New Run modal outside the lit rectangle.
 * (Chromium happened to scroll the modal anyway — its compositor picks the
 * target by layer — but that is a engine accident, not a rule to lean on.)
 *
 * A tour only ever forbids the click. The gesture is relayed by hand: find the
 * app element under the pointer *as if the overlay were not there*, climb to the
 * nearest thing that can scroll, and move it by the wheel's delta.
 */

/** Pixels per line for `WheelEvent.DOM_DELTA_LINE` — the usual browser default. */
const LINE_PX = 16;

/** Can this element scroll along at least one axis right now? */
export function isScrollable(el: Element): boolean {
  if (typeof window === "undefined") return false;
  const style = window.getComputedStyle(el);
  const scrolls = (overflow: string) => overflow === "auto" || overflow === "scroll" || overflow === "overlay";
  const vertically = scrolls(style.overflowY) && el.scrollHeight > el.clientHeight;
  const horizontally = scrolls(style.overflowX) && el.scrollWidth > el.clientWidth;
  return vertically || horizontally;
}

/**
 * The nearest scrollable element from `start` upwards, the document's scrolling
 * element as the last resort (the page itself scrolls when nothing else does).
 */
export function scrollableAncestor(start: Element | null): Element | null {
  let el: Element | null = start;
  while (el && el !== document.documentElement && el !== document.body) {
    if (isScrollable(el)) return el;
    el = el.parentElement;
  }
  return document.scrollingElement ?? document.documentElement;
}

/**
 * The top-most element under (x, y) that is NOT part of `overlay` — the app the
 * dim is drawn over. `null` when the overlay is all there is at that point.
 */
export function elementBeneath(overlay: Element, x: number, y: number): Element | null {
  if (typeof document.elementsFromPoint !== "function") return null;
  for (const el of document.elementsFromPoint(x, y)) {
    if (!overlay.contains(el)) return el;
  }
  return null;
}

/** The wheel's delta in pixels, whatever unit the browser reported it in. */
export function wheelDeltaPx(
  e: { deltaX: number; deltaY: number; deltaMode: number },
  page: { width: number; height: number },
): { dx: number; dy: number } {
  switch (e.deltaMode) {
    case 1: // DOM_DELTA_LINE
      return { dx: e.deltaX * LINE_PX, dy: e.deltaY * LINE_PX };
    case 2: // DOM_DELTA_PAGE
      return { dx: e.deltaX * page.width, dy: e.deltaY * page.height };
    default:
      return { dx: e.deltaX, dy: e.deltaY };
  }
}

/**
 * Relay one wheel event that landed on the overlay to whatever scrolls beneath
 * it. Returns the element that was scrolled, `null` when nothing was.
 */
export function relayWheel(overlay: Element, e: WheelEvent): Element | null {
  const under = elementBeneath(overlay, e.clientX, e.clientY);
  if (!under) return null;
  const scroller = scrollableAncestor(under);
  if (!scroller) return null;
  const { dx, dy } = wheelDeltaPx(e, { width: scroller.clientWidth, height: scroller.clientHeight });
  scroller.scrollTop += dy;
  scroller.scrollLeft += dx;
  return scroller;
}
