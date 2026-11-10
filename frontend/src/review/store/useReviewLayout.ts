import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { DiffFile } from "../../shared/generated/DiffFile";
import { defaultCollapsed, filePath, readListOpen, readView, writeListOpen, writeView } from "../../run/model/runRefs";
import type { ViewMode } from "../../run/model/runRefs";
import type { Anchor } from "../model/drafts";
import { allDirs, ancestorDirs, buildFileTree, visibleRows } from "../model/fileTree";
import { toCss } from "../../shared/ui/zoom";

const NARROW_QUERY = "(max-width: 900px)";
/** What the reader does that ends a landing's pinning. */
const READER_MOVES = ["wheel", "touchstart", "pointerdown", "keydown"] as const;

/**
 * How the Review page lays its files out (#749): split or unified (remembered
 * on this browser, unified on a narrow window), the file list open or not, its
 * filter, which files are collapsed, which one is current (scroll-spy), and the
 * moves between files, hunks and anchored lines.
 *
 * CORE-84: the list is a folder tree (`fileTree`); `files` come in its order
 * (`treeOrder`), so the list and the cards read the same way. The collapsed
 * folders survive a change of current file; `j` / `k` step through the files
 * the list shows (a collapsed folder's are skipped), `←` / `→` fold and unfold
 * the folders around the current file.
 */
/** `viewed`: the files marked « Viewed » for the pair — they open folded. */
export function useReviewLayout(files: DiffFile[], viewed?: ReadonlySet<string>) {
  const [view, setView] = useState<ViewMode>(() => readView());
  const [narrow, setNarrow] = useState<boolean>(() => window.matchMedia?.(NARROW_QUERY).matches ?? false);
  const [listOpen, setListOpen] = useState<boolean>(() => readListOpen());
  const [filter, setFilter] = useState("");
  const [collapsed, setCollapsed] = useState<Set<string> | null>(null);
  const [currentIdx, setCurrentIdx] = useState(0);
  const [collapsedDirs, setCollapsedDirs] = useState<ReadonlySet<string>>(() => new Set());
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

  /** A new pair: default collapse (the large diff, the binaries, the files
   *  viewed), first file, back to the top. */
  const resetForPair = () => {
    setCollapsed(null);
    setCurrentIdx(0);
  };
  const scrollTop = () => mainRef.current?.scrollTo?.({ top: 0 });

  const collapsedSet = useMemo(() => {
    if (collapsed) return collapsed;
    const init = defaultCollapsed(files);
    for (const f of files) if (viewed?.has(filePath(f))) init.add(filePath(f));
    return init;
  }, [collapsed, files, viewed]);
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

  const tree = useMemo(() => buildFileTree(files.map(filePath)), [files]);
  const rows = useMemo(() => visibleRows(tree, collapsedDirs, filter), [tree, collapsedDirs, filter]);
  const toggleDir = useCallback((path: string) => {
    setCollapsedDirs((cur) => {
      const next = new Set(cur);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, []);
  const dirs = useMemo(() => allDirs(tree), [tree]);
  const allDirsCollapsed = dirs.length > 0 && dirs.every((d) => collapsedDirs.has(d));
  const toggleAllDirs = () => setCollapsedDirs(allDirsCollapsed ? new Set() : new Set(dirs));
  /** `←`: fold the innermost open folder holding the current file; `→`: unfold the outermost folded one. */
  const foldCurrent = useCallback(
    (dir: "fold" | "unfold") => {
      const f = files[currentIdx];
      if (!f) return;
      const around = ancestorDirs(tree, filePath(f));
      const target =
        dir === "fold" ? [...around].reverse().find((d) => !collapsedDirs.has(d)) : around.find((d) => collapsedDirs.has(d));
      if (target) toggleDir(target);
    },
    [files, currentIdx, tree, collapsedDirs, toggleDir],
  );

  /** While a landing pins its file, every card (re)mounted meanwhile is watched too. */
  const pinObserver = useRef<ResizeObserver | null>(null);
  const registerEl = useCallback((path: string, el: HTMLDivElement | null) => {
    if (el) {
      cardEls.current.set(path, el);
      pinObserver.current?.observe(el);
    } else cardEls.current.delete(path);
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

  /** Make a file current, expanding it first; its path, or null for no such file. */
  const select = useCallback(
    (index: number): string | null => {
      const f = files[index];
      if (!f) return null;
      const path = filePath(f);
      if (collapsedSet.has(path)) {
        const next = new Set(collapsedSet);
        next.delete(path);
        setCollapsed(next);
      }
      setCurrentIdx(index);
      return path;
    },
    [files, collapsedSet],
  );

  const goFile = useCallback(
    (index: number) => {
      const path = select(index);
      if (path === null) return;
      const card = cardEls.current.get(path);
      const el = mainRef.current;
      if (card && el) el.scrollTo?.({ top: Math.max(0, card.offsetTop - 8), behavior: "smooth" });
    },
    [select],
  );

  /**
   * Land on a file (a Review link) and keep it at the top while the diff
   * settles: the cards above it grow as their body renders, so the page
   * re-aligns on every resize until the reader scrolls, clicks or types, or a
   * few seconds pass.
   */
  const stopPinning = useRef<(() => void) | null>(null);
  const landOnFile = useCallback(
    (index: number) => {
      stopPinning.current?.();
      const main = mainRef.current;
      const path = main ? select(index) : null;
      if (!main || path === null) return;
      const align = () => {
        const card = cardEls.current.get(path);
        if (card) main.scrollTop = Math.max(0, card.offsetTop - 8);
      };
      align();
      if (typeof ResizeObserver === "undefined") return;
      const ro = new ResizeObserver(align);
      cardEls.current.forEach((el) => ro.observe(el));
      pinObserver.current = ro;
      const stop = () => {
        ro.disconnect();
        pinObserver.current = null;
        window.clearTimeout(timer);
        for (const ev of READER_MOVES) window.removeEventListener(ev, stop, true);
        stopPinning.current = null;
      };
      const timer = window.setTimeout(stop, 5000);
      for (const ev of READER_MOVES) window.addEventListener(ev, stop, { capture: true, passive: true });
      stopPinning.current = stop;
    },
    [select],
  );
  useEffect(() => () => stopPinning.current?.(), []);

  /** `j` / `k`: the next / previous file row the list shows. */
  const stepFile = useCallback(
    (dir: 1 | -1) => {
      const shown = rows.flatMap((r) => (r.node.kind === "file" ? [r.node.index] : []));
      const next = dir === 1 ? shown.find((i) => i > currentIdx) : [...shown].reverse().find((i) => i < currentIdx);
      if (next !== undefined) goFile(next);
    },
    [rows, currentIdx, goFile],
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
    // The rects are screen pixels; the scroll is in the page's CSS pixels (UI scale).
    if (target) el.scrollBy?.({ top: toCss(target.getBoundingClientRect().top - base, el), behavior: "smooth" });
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
    landOnFile,
    stepFile,
    goHunk,
    jumpTo,
    rows,
    toggleDir,
    toggleAllDirs,
    allDirsCollapsed,
    hasDirs: dirs.length > 0,
    foldCurrent,
  };
}
