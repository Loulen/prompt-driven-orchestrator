import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { usePanelRef } from "react-resizable-panels";

/**
 * #949 — the right pane collapses when it has nothing to show (the router's
 * `collapsed`) and reopens, instantly, at the last width the user chose. The
 * panel is `collapsible` ONLY while it is (or is about to be) collapsed: a
 * collapsible panel also collapses when dragged under its min size, and there
 * is no manual collapse gesture. The library only reads a panel's new
 * constraints once its group re-registers — in a re-render the layout effect
 * does not wait for — so `syncRightPane` is idempotent and retries in a
 * microtask (still before paint: no frame of the stale pane) until the
 * collapse lands. Expanding resizes to the PERSISTED width, not the library's
 * in-memory one, so it also holds after a reload; the persisted width itself
 * never records the collapse (`useResizableLayout`).
 */
export function useCollapsibleRightPane(
  collapsed: boolean,
  persistedSize: (panelId: string) => number | undefined,
  defaultSize: number,
) {
  const panelRef = usePanelRef();
  const collapsedRef = useRef(collapsed);
  const [collapsible, setCollapsible] = useState(collapsed);

  const sync = useCallback(
    function sync(attempt = 0) {
      const panel = panelRef.current;
      if (!panel) return;
      let settled = false;
      try {
        if (collapsedRef.current) {
          if (!panel.isCollapsed()) panel.collapse();
          settled = panel.isCollapsed();
        } else {
          if (panel.isCollapsed()) {
            panel.resize(`${persistedSize("right") ?? defaultSize}%`);
          }
          settled = true;
        }
      } catch {
        // The group is between two registrations — retried below.
      }
      if (!settled && attempt < 5) queueMicrotask(() => sync(attempt + 1));
    },
    [panelRef, persistedSize, defaultSize],
  );

  useLayoutEffect(() => {
    collapsedRef.current = collapsed;
    if (collapsed) {
      // No-op until the collapsible constraint lands — `sync` retries.
      setCollapsible(true);
      sync();
    } else {
      // Expand first, while still collapsible, THEN drop the constraint: a
      // non-collapsible panel at 0% would be clamped to its min size.
      sync();
      setCollapsible(false);
    }
  }, [collapsed, sync]);

  return { panelRef, collapsible };
}
