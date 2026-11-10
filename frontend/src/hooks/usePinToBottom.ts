import { useCallback, useEffect, useRef, useState } from "react";

const THRESHOLD = 8;

export function isAtBottom(el: {
  scrollHeight: number;
  scrollTop: number;
  clientHeight: number;
}): boolean {
  return el.scrollHeight - el.scrollTop - el.clientHeight < THRESHOLD;
}

export function isAtTop(el: { scrollTop: number }): boolean {
  return el.scrollTop < THRESHOLD;
}

/** Which end of a scrolled list the newest content arrives at. */
export type PinEdge = "top" | "bottom";

/**
 * Whether the reader sits at the end of a list where new content arrives —
 * the bottom of a log, or the top of a list shown newest first — measured on
 * every scroll and resize, reset to « pinned » when the node or the iteration
 * changes. `scrollToEnd` brings the reader back there.
 */
export function usePinToEnd(
  ref: React.RefObject<HTMLElement | null>,
  nodeId: string,
  iter: number,
  edge: PinEdge,
) {
  const [pinned, setPinned] = useState(true);
  const pinnedRef = useRef(true);

  useEffect(() => {
    pinnedRef.current = pinned;
  }, [pinned]);

  const [resetKey, setResetKey] = useState(`${nodeId}:${iter}`);
  const currentKey = `${nodeId}:${iter}`;
  if (resetKey !== currentKey) {
    setResetKey(currentKey);
    setPinned(true);
  }

  const atEnd = useCallback((el: HTMLElement) => (edge === "top" ? isAtTop(el) : isAtBottom(el)), [edge]);

  const handleScroll = useCallback(() => {
    const el = ref.current;
    if (!el) return;
    const next = atEnd(el);
    pinnedRef.current = next;
    setPinned(next);
  }, [ref, atEnd]);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const observer = new ResizeObserver(() => {
      const target = ref.current;
      if (!target) return;
      setPinned(atEnd(target));
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [ref, atEnd]);

  const scrollToEnd = useCallback(() => {
    const el = ref.current;
    if (el) {
      el.scrollTop = edge === "top" ? 0 : el.scrollHeight;
    }
    pinnedRef.current = true;
    setPinned(true);
  }, [ref, edge]);

  return { pinned, pinnedRef, handleScroll, scrollToEnd };
}

export function usePinToBottom(
  ref: React.RefObject<HTMLElement | null>,
  nodeId: string,
  iter: number,
) {
  const { pinned, pinnedRef, handleScroll, scrollToEnd } = usePinToEnd(ref, nodeId, iter, "bottom");
  return { pinnedToBottom: pinned, pinnedRef, handleScroll, scrollToBottom: scrollToEnd };
}
