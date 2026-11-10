/**
 * Bringing a tour step's target in front of the reader (#824, #825): scrolled
 * into view when any part of it is cut off, or — a canvas card — asked of the
 * canvas. Reads the DOM, scrolls; decides nothing about *when* (the pump keys it
 * on the aim, so the reader's own scrolling stays theirs).
 */

import { canvasNodeIdOf, revealCanvasNode } from "../../lib/canvasReveal";

interface Box {
  top: number;
  left: number;
  bottom: number;
  right: number;
}

/** Half a pixel of rounding is not a cut. */
const EPSILON = 0.5;

function inside(inner: Box, outer: Box): boolean {
  return (
    inner.top >= outer.top - EPSILON &&
    inner.left >= outer.left - EPSILON &&
    inner.bottom <= outer.bottom + EPSILON &&
    inner.right <= outer.right + EPSILON
  );
}

/** An ancestor that cuts what overflows it: a scroll pane, a clipped panel. */
function clips(el: Element): boolean {
  const style = getComputedStyle(el);
  return [style.overflow, style.overflowX, style.overflowY].some(
    (v) => v === "hidden" || v === "auto" || v === "scroll" || v === "clip",
  );
}

/**
 * Whether any part of `el` is cut off: past an edge of the window, on either
 * axis, or past the box of an ancestor that clips. The vertical window test
 * alone left the inspector's « Add output » `+` cut by the window's right edge
 * on *First pipeline* (FP of CORE-227), lit and half there.
 */
export function cutOff(el: Element): boolean {
  const r = el.getBoundingClientRect();
  const window_ = { top: 0, left: 0, bottom: window.innerHeight, right: window.innerWidth };
  if (!inside(r, window_)) return true;
  for (let a = el.parentElement; a && a !== document.body; a = a.parentElement) {
    if (clips(a) && !inside(r, a.getBoundingClientRect())) return true;
  }
  return false;
}

/**
 * Put the target where the reader can act on it. A canvas card cannot be
 * scrolled to: it is absolutely positioned inside a transformed viewport, so
 * `scrollIntoView` on it moves nothing (#825, FP iteration 2) — the canvas is
 * asked instead, and decides for itself whether the card is already in frame.
 * Anything else is scrolled only when cut off, on both axes, through every
 * scrolling ancestor.
 */
export function revealTarget(el: Element): void {
  const nodeId = canvasNodeIdOf(el);
  if (nodeId) {
    revealCanvasNode(nodeId);
    return;
  }
  if (cutOff(el)) el.scrollIntoView({ block: "center", inline: "nearest", behavior: "smooth" });
}
