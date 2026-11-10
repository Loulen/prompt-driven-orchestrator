import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { pickSection } from "./scrollSpyPick";

/**
 * Scroll-spy for the Settings sub-column (#690): which section of the scrolling page is
 * "in view". Two rules make it honest:
 *
 * - The section whose heading is nearest the top of the container wins (an
 *   `IntersectionObserver` with `rootMargin: -20% 0 -60% 0`). When the container is
 *   scrolled to the bottom, the section that fills most of the view wins, even if its
 *   heading never reaches the top: a long last section is lit there (the classic "last
 *   section never highlights" bug), a short one under a long one is not
 *   (`scrollSpyPick.ts`).
 * - A clicked entry is **pinned** until the smooth scroll settles (`scrollend`, or 150 ms
 *   after the last `scroll` event), so the highlight never flickers through the sections
 *   the scroll passes over — and it stays on the clicked entry once settled, even when the
 *   page could not scroll it to the top (#691: three sections on a short page).
 *
 * The page grows after mount (#691: inline panels fetch their own data), which moves every
 * heading and can flip the "scrolled to the bottom" rule: a `ResizeObserver` on the
 * container's content re-evaluates the pick whenever its height changes.
 *
 * Environments without `IntersectionObserver` (jsdom) keep the pinned / initial value.
 */
export function useScrollSpy<Id extends string>(
  sectionIds: readonly Id[],
  containerRef: RefObject<HTMLElement | null>,
  enabled: boolean,
): { active: Id; scrollTo: (id: Id) => void } {
  const [spied, setSpied] = useState<Id>(sectionIds[0]);
  const [pinned, setPinned] = useState<Id | null>(null);
  const pinnedRef = useRef<Id | null>(null);
  const settleTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const ids = sectionIds.join("\u0000");

  useEffect(() => {
    if (!enabled) return;
    const container = containerRef.current;
    if (!container) return;

    const list = ids.split("\u0000") as Id[];
    const pick = () => {
      if (pinnedRef.current) return;
      // At the bottom by the scroll metrics, or by the boxes: the last section's end is in
      // view. The boxes agree under the UI scale's `zoom`, where the metrics may not.
      const box = container.getBoundingClientRect();
      const last = container.querySelector<HTMLElement>(`[data-section-id="${list[list.length - 1]}"]`);
      const atBottom =
        container.scrollTop + container.clientHeight >= container.scrollHeight - 2 ||
        (last != null && last.getBoundingClientRect().bottom <= box.bottom + 2);
      const sections = list.flatMap((id) => {
        const el = container.querySelector<HTMLElement>(`[data-section-id="${id}"]`);
        if (!el) return [];
        const r = el.getBoundingClientRect();
        return [{ id, top: r.top, bottom: r.bottom }];
      });
      const best = pickSection(
        sections,
        // The boxes' own height: the sections are measured in it, under the UI scale's `zoom` too.
        { top: box.top, height: box.height },
        atBottom && container.scrollHeight > container.clientHeight,
      );
      if (best) setSpied(best);
    };

    // A click that settled keeps its entry: on a short page the target may sit at the very
    // bottom, where the "last section wins" rule would otherwise override the user's own
    // choice. The next user scroll (or content growth) hands the pick back to the spy.
    const unpin = () => {
      const clicked = pinnedRef.current;
      pinnedRef.current = null;
      setPinned(null);
      if (clicked) setSpied(clicked);
      else pick();
    };
    const onScroll = () => {
      if (pinnedRef.current) {
        if (settleTimer.current) clearTimeout(settleTimer.current);
        settleTimer.current = setTimeout(unpin, 150);
        return;
      }
      pick();
    };
    const onScrollEnd = () => {
      if (!pinnedRef.current) return;
      if (settleTimer.current) clearTimeout(settleTimer.current);
      unpin();
    };

    container.addEventListener("scroll", onScroll, { passive: true });
    container.addEventListener("scrollend", onScrollEnd);

    let observer: IntersectionObserver | null = null;
    if (typeof IntersectionObserver !== "undefined") {
      observer = new IntersectionObserver(() => pick(), {
        root: container,
        rootMargin: "-20% 0px -60% 0px",
        threshold: [0, 1],
      });
      for (const id of list) {
        const el = container.querySelector(`[data-section-id="${id}"]`);
        if (el) observer.observe(el);
      }
      pick();
    }

    let resize: ResizeObserver | null = null;
    if (typeof ResizeObserver !== "undefined") {
      resize = new ResizeObserver(() => pick());
      // The content wrapper grows, not the container (which is the flex-sized viewport).
      for (const child of Array.from(container.children)) resize.observe(child);
    }

    return () => {
      container.removeEventListener("scroll", onScroll);
      container.removeEventListener("scrollend", onScrollEnd);
      observer?.disconnect();
      resize?.disconnect();
      if (settleTimer.current) clearTimeout(settleTimer.current);
    };
  }, [ids, containerRef, enabled]);

  const scrollTo = useCallback(
    (id: Id) => {
      pinnedRef.current = id;
      setPinned(id);
      const container = containerRef.current;
      const el = container?.querySelector<HTMLElement>(`[data-section-id="${id}"]`);
      // jsdom has no `scrollIntoView`; the pin alone then carries the highlight.
      el?.scrollIntoView?.({ behavior: "smooth", block: "start" });
      // No scroll will happen when the section is already in place (or the page does not
      // overflow): release the pin after the settle delay so the spy resumes.
      if (settleTimer.current) clearTimeout(settleTimer.current);
      settleTimer.current = setTimeout(() => {
        pinnedRef.current = null;
        setPinned(null);
        setSpied(id);
      }, 600);
    },
    [containerRef],
  );

  return { active: pinned ?? spied, scrollTo };
}
