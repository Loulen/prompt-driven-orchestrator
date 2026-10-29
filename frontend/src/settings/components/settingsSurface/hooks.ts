/** The Settings surface's hooks: what a search finds, and the Save shortcut. */

import { useCallback, useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";

/**
 * Under a search: how many setting rows show, and how many sections. A section with no
 * row found and whose name is not found hides; read from the page after each change.
 */
export function useFound(pagesRef: RefObject<HTMLDivElement | null>, active: boolean) {
  const [found, setFound] = useState({ rows: 0, sections: 1 });
  const measure = useCallback(() => {
    const pages = pagesRef.current;
    if (!pages) return;
    let sections = 0;
    for (const section of pages.querySelectorAll<HTMLElement>(".set-sec")) {
      const show = !active || !!section.dataset.labelMatch || !!section.querySelector(".set-row");
      section.hidden = !show;
      if (show) sections += 1;
    }
    const rows = pages.querySelectorAll(".set-row").length;
    setFound((current) =>
      current.rows === rows && current.sections === sections ? current : { rows, sections },
    );
  }, [pagesRef, active]);
  useLayoutEffect(measure);
  useEffect(() => {
    const pages = pagesRef.current;
    if (!active || !pages || typeof MutationObserver === "undefined") return;
    const observer = new MutationObserver(measure);
    observer.observe(pages, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, [pagesRef, active, measure]);
  return found;
}

/** Ctrl+S (Cmd+S) runs `save` while the surface is open; `null`: the shortcut does nothing. */
export function useSaveShortcut(open: boolean, save: (() => Promise<boolean>) | null) {
  const saveNow = useRef(save);
  useLayoutEffect(() => {
    saveNow.current = save;
  });
  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "s" || !(event.ctrlKey || event.metaKey) || event.defaultPrevented) return;
      if (!saveNow.current) return;
      event.preventDefault();
      void saveNow.current();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);
}
