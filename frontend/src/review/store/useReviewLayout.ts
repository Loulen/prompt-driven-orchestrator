import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { DiffFile } from "../../shared/generated/DiffFile";
import { defaultCollapsed, filePath, readListOpen, readView, writeListOpen, writeView } from "../../run/model/runRefs";
import type { ViewMode } from "../../run/model/runRefs";
import type { Anchor } from "../model/drafts";

const NARROW_QUERY = "(max-width: 900px)";

/**
 * How the Review page lays its files out (#749): split or unified (remembered
 * on this browser, unified on a narrow window), the file list open or not, its
 * filter, which files are collapsed, which one is current (scroll-spy), and the
 * moves between files, hunks and anchored lines.
 */
export function useReviewLayout(files: DiffFile[]) {
  const [view, setView] = useState<ViewMode>(() => readView());
  const [narrow, setNarrow] = useState<boolean>(() => window.matchMedia?.(NARROW_QUERY).matches ?? false);
  const [listOpen, setListOpen] = useState<boolean>(() => readListOpen());
  const [filter, setFilter] = useState("");
  const [collapsed, setCollapsed] = useState<Set<string> | null>(null);
  const [currentIdx, setCurrentIdx] = useState(0);
  const mainRef = useRef<HTMLDivElement | null>(null);
  const filterRef = useRef<HTMLInputElement | null>(null);
  const cardEls = useRef<Map<string, HTMLDivElement>>(new Map());

  const effectiveView: ViewMode = narrow ? "unified" : view;
  const changeView = (v: ViewMode) => {
    setView(v);
    writeView(v);
  };
  const toggleList = () => {
    setListOpen((o) => {
      writeListOpen(!o);
      return !o;
    });
  };
  useEffect(() => {
    const mq = window.matchMedia?.(NARROW_QUERY);
    if (!mq) return;
    const onChange = () => setNarrow(mq.matches);
    mq.addEventListener?.("change", onChange);
    return () => mq.removeEventListener?.("change", onChange);
  }, []);

  /** A new pair: default collapse, first file, back to the top. */
  const resetForPair = () => {
    setCollapsed(null);
    setCurrentIdx(0);
  };
  const scrollTop = () => mainRef.current?.scrollTo?.({ top: 0 });

  const collapsedSet = useMemo(() => collapsed ?? defaultCollapsed(files), [collapsed, files]);
  const toggleFile = (path: string) => {
    const next = new Set(collapsedSet);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    setCollapsed(next);
  };
  const collapseAll = () => {
    const anyOpen = files.some((f) => !collapsedSet.has(filePath(f)));
    setCollapsed(anyOpen ? new Set(files.map(filePath)) : new Set());
  };

  const registerEl = useCallback((path: string, el: HTMLDivElement | null) => {
    if (el) cardEls.current.set(path, el);
    else cardEls.current.delete(path);
  }, []);

  const onScroll = useCallback(() => {
    const el = mainRef.current;
    if (!el) return;
    const top = el.scrollTop;
    let idx = 0;
    files.forEach((f, i) => {
      const card = cardEls.current.get(filePath(f));
      if (card && card.offsetTop <= top + 40) idx = i;
    });
    setCurrentIdx(idx);
  }, [files]);

  const goFile = useCallback(
    (index: number) => {
      const f = files[index];
      if (!f) return;
      const path = filePath(f);
      if (collapsedSet.has(path)) {
        const next = new Set(collapsedSet);
        next.delete(path);
        setCollapsed(next);
      }
      setCurrentIdx(index);
      const card = cardEls.current.get(path);
      const el = mainRef.current;
      if (card && el) el.scrollTo?.({ top: Math.max(0, card.offsetTop - 8), behavior: "smooth" });
    },
    [files, collapsedSet],
  );

  const goHunk = useCallback((dir: 1 | -1) => {
    const el = mainRef.current;
    if (!el) return;
    const rows = Array.from(el.querySelectorAll<HTMLElement>('tr[data-state="hunk"]'));
    const base = el.getBoundingClientRect().top + 48;
    const target =
      dir === 1
        ? rows.find((r) => r.getBoundingClientRect().top > base + 4)
        : [...rows].reverse().find((r) => r.getBoundingClientRect().top < base - 4);
    if (target) el.scrollBy?.({ top: target.getBoundingClientRect().top - base, behavior: "smooth" });
  }, []);

  /** Scroll the anchored line of an entry into view, expanding its file first. */
  const jumpTo = useCallback(
    (anchor: Anchor) => {
      if (collapsedSet.has(anchor.path)) {
        const next = new Set(collapsedSet);
        next.delete(anchor.path);
        setCollapsed(next);
      }
      const attempt = (left: number) => {
        const card = cardEls.current.get(anchor.path);
        const el =
          card?.querySelector<HTMLElement>(`td.diff-line-${anchor.side}-num > span[data-line-num="${anchor.line}"]`) ??
          card?.querySelector<HTMLElement>(`span[data-line-${anchor.side}-num="${anchor.line}"]`);
        if (el) el.closest("tr")?.scrollIntoView?.({ behavior: "smooth", block: "center" });
        else if (card && left > 0) window.setTimeout(() => attempt(left - 1), 60);
        else if (card) {
          const main = mainRef.current;
          if (main) main.scrollTo?.({ top: Math.max(0, card.offsetTop - 8), behavior: "smooth" });
        }
      };
      attempt(5);
    },
    [collapsedSet],
  );

  return {
    view,
    narrow,
    effectiveView,
    changeView,
    listOpen,
    toggleList,
    filter,
    setFilter,
    filterRef,
    mainRef,
    collapsedSet,
    toggleFile,
    collapseAll,
    resetForPair,
    scrollTop,
    currentIdx,
    registerEl,
    onScroll,
    goFile,
    goHunk,
    jumpTo,
  };
}
