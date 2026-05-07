import { useCallback, useEffect, useRef, useState } from "react";

const THRESHOLD = 8;

export function isAtBottom(el: {
  scrollHeight: number;
  scrollTop: number;
  clientHeight: number;
}): boolean {
  return el.scrollHeight - el.scrollTop - el.clientHeight < THRESHOLD;
}

export function usePinToBottom(
  ref: React.RefObject<HTMLElement | null>,
  nodeId: string,
  iter: number,
) {
  const [pinnedToBottom, setPinnedToBottom] = useState(true);
  const pinnedRef = useRef(true);

  useEffect(() => {
    pinnedRef.current = pinnedToBottom;
  }, [pinnedToBottom]);

  const [resetKey, setResetKey] = useState(`${nodeId}:${iter}`);
  const currentKey = `${nodeId}:${iter}`;
  if (resetKey !== currentKey) {
    setResetKey(currentKey);
    setPinnedToBottom(true);
  }

  const handleScroll = useCallback(() => {
    const el = ref.current;
    if (!el) return;
    setPinnedToBottom(isAtBottom(el));
  }, [ref]);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const observer = new ResizeObserver(() => {
      const target = ref.current;
      if (!target) return;
      setPinnedToBottom(isAtBottom(target));
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [ref]);

  const scrollToBottom = useCallback(() => {
    const el = ref.current;
    if (el) {
      el.scrollTop = el.scrollHeight;
    }
    setPinnedToBottom(true);
  }, [ref]);

  return { pinnedToBottom, pinnedRef, handleScroll, scrollToBottom };
}
