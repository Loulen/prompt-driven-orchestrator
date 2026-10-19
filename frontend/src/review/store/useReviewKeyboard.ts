import { useEffect } from "react";
import type { useReviewLayout } from "./useReviewLayout";

/**
 * The Review page's keys (#749, #751, CORE-84): `j` / `k` file, `n` / `p` hunk,
 * `c` / `C` comment, `u` view, `[` list, `/` filter, `←` / `→` fold the folders
 * around the current file. Ignored while typing (Escape leaves the field) and
 * with a modifier held.
 */
export function useReviewKeyboard(layout: ReturnType<typeof useReviewLayout>, jumpComment: (dir: 1 | -1) => void) {
  const { view, listOpen, filter, stepFile, foldCurrent, goHunk, changeView, toggleList, filterRef } = layout;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) {
        if (e.key === "Escape") t.blur();
        return;
      }
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      switch (e.key) {
        case "c":
          jumpComment(1);
          break;
        case "C":
          jumpComment(-1);
          break;
        case "j":
          stepFile(1);
          break;
        case "k":
          stepFile(-1);
          break;
        // Folding is the list's: not while it is hidden or filtered (the arrows then only scroll).
        case "ArrowLeft":
        case "ArrowRight":
          if (!listOpen || filter.trim()) return;
          foldCurrent(e.key === "ArrowLeft" ? "fold" : "unfold");
          break;
        case "n":
          goHunk(1);
          break;
        case "p":
          goHunk(-1);
          break;
        case "u":
          changeView(view === "split" ? "unified" : "split");
          break;
        case "[":
          toggleList();
          break;
        case "/":
          e.preventDefault();
          if (!listOpen) toggleList();
          window.setTimeout(() => filterRef.current?.focus(), 0);
          break;
        default:
          return;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // The view and list toggles are re-created every render; they read state.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [view, listOpen, filter, stepFile, foldCurrent, goHunk, jumpComment]);
}
