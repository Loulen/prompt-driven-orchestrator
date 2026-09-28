import { useCallback, useMemo, useSyncExternalStore } from "react";
import type { RunState } from "../../shared/generated/RunState";
import { readSeen, seenKey, unreadCommentCount } from "../model/seen";
import type { SeenMap } from "../model/seen";

/**
 * How many of a Run's sent review comments carry a reply this browser has not
 * seen (#751) — the Diff tab's count badge. The "seen" marker lives in
 * localStorage (written by the Review page: opening it marks everything seen,
 * looking at a card marks that card), so the count follows both the Run's
 * WebSocket pushes (through `run`) and the other tab's `storage` events.
 */
export function useReviewUnread(run: RunState | null): number {
  const seen = useReviewSeen(run);
  return useMemo(() => (run ? unreadCommentCount(run.review_comments, seen) : 0), [run, seen]);
}

/**
 * The per-browser "seen" marker of a Run's review replies (#751 / #752), live:
 * follows the other tab's `storage` events. The toolbar's Review pill reads its
 * tone from it (solid blue while a reply is unread).
 */
export function useReviewSeen(run: RunState | null): SeenMap {
  const runId = run?.run_id ?? null;
  const subscribe = useCallback((onChange: () => void) => {
    window.addEventListener("storage", onChange);
    return () => window.removeEventListener("storage", onChange);
  }, []);
  // The raw string is the snapshot (stable between writes), parsed below.
  const raw = useSyncExternalStore(
    subscribe,
    () => {
      if (!runId) return null;
      try {
        return localStorage.getItem(seenKey(runId));
      } catch {
        return null;
      }
    },
    () => null,
  );
  return useMemo(() => (runId ? readSeen(runId, { getItem: () => raw }) : {}), [runId, raw]);
}
