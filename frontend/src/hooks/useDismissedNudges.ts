import { useCallback, useState } from "react";
import { loadDismissed, saveDismissed } from "../lib/dismissedBanners";

/**
 * Tracks which advisory fan-out nudges the user has dismissed for one pipeline
 * tab (#268), backed by `localStorage` (`dismissedBanners`). A localStorage
 * write alone does not re-render, so the dismissed set lives in React state and
 * is mirrored to storage on every dismiss; the gate that decides whether the
 * banner shows reads this state.
 *
 * Keyed by `tabId` (the pipeline id for edit tabs — banners never render on run
 * tabs). When the active tab switches, the set reloads for the new pipeline so
 * one pipeline's dismissals never leak into another's banner.
 */
export function useDismissedNudges(tabId: string) {
  const [dismissed, setDismissed] = useState<Set<string>>(() => loadDismissed(tabId));

  // Reload when switching to a different pipeline tab (stale-state guard). This
  // is React's "adjust state during render on a prop change" pattern (tracking
  // the previous tabId) rather than an effect — it reloads before paint with no
  // extra render, and is the form the react-hooks lint rule sanctions.
  const [prevTabId, setPrevTabId] = useState(tabId);
  if (prevTabId !== tabId) {
    setPrevTabId(tabId);
    setDismissed(loadDismissed(tabId));
  }

  const dismiss = useCallback(
    (id: string) => {
      setDismissed((prev) => {
        if (prev.has(id)) return prev; // stable ref on no-op → no needless re-render
        const next = new Set(prev).add(id);
        saveDismissed(tabId, next);
        return next;
      });
    },
    [tabId],
  );

  return { dismissed, dismiss };
}
