import { useEffect, type RefObject } from "react";
import { appZoom } from "../shared/ui/zoom";

/**
 * Where a strip scrolled to `scrollLeft`, `viewWidth` wide, scrolls to show
 * the whole of an item at `itemLeft` (in the strip's content), `itemWidth`
 * wide: unchanged when it already shows, else just enough (its start first
 * when it is wider than the strip).
 */
export function revealScroll(scrollLeft: number, viewWidth: number, itemLeft: number, itemWidth: number): number {
  if (itemLeft < scrollLeft) return itemLeft;
  const itemRight = itemLeft + itemWidth;
  if (itemRight > scrollLeft + viewWidth) return Math.min(itemLeft, itemRight - viewWidth);
  return scrollLeft;
}

function reveal(list: HTMLElement, behavior: ScrollBehavior) {
  const item = list.querySelector<HTMLElement>("[data-active]");
  if (!item) return;
  // Rects are in screen pixels, scrolling in the strip's CSS pixels.
  const zoom = appZoom(list);
  const listRect = list.getBoundingClientRect();
  const itemRect = item.getBoundingClientRect();
  const itemLeft = (itemRect.left - listRect.left) / zoom + list.scrollLeft;
  const left = revealScroll(list.scrollLeft, list.clientWidth, itemLeft, itemRect.width / zoom);
  if (left !== list.scrollLeft) list.scrollTo({ left, behavior });
}

/**
 * Keeps the active tab of a tab strip (its `[data-active]` child) in view:
 * when another one becomes active (`activeKey`), whichever kind of tab it is,
 * and when the strip changes width (a pane opening beside it, a resize).
 */
export function useActiveTabInView(ref: RefObject<HTMLElement | null>, activeKey: string | null) {
  useEffect(() => {
    if (ref.current && activeKey != null) reveal(ref.current, "smooth");
  }, [ref, activeKey]);

  useEffect(() => {
    const list = ref.current;
    if (!list || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => reveal(list, "auto"));
    observer.observe(list);
    return () => observer.disconnect();
  }, [ref]);
}
