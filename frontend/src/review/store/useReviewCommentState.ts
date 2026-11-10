import { useCallback, useEffect, useRef, useState } from "react";
import { SHOW_OUTDATED_KEY } from "../model/entries";
import { readDrafts, writeDrafts } from "../model/drafts";
import type { ReviewDraft } from "../model/drafts";
import { readSeen } from "../model/seen";
import type { SeenMap } from "../model/seen";

const SHOW_RESOLVED_KEY = "pdo.review.showResolved";

/** A remembered boolean toggle of this browser (on by default). */
function useStoredToggle(key: string): [boolean, () => void] {
  const [on, setOn] = useState<boolean>(() => {
    try {
      return localStorage.getItem(key) !== "false";
    } catch {
      return true;
    }
  });
  const toggle = () => {
    setOn((v) => {
      try {
        localStorage.setItem(key, String(!v));
      } catch {
        // Remembered for the session only.
      }
      return !v;
    });
  };
  return [on, toggle];
}

/**
 * What the Review page keeps about its comments (#750, #751, #752): the drafts
 * of this browser (localStorage per Run), the one opened for edit, the ones
 * travelling in a send, the Resolve / Reopen in flight, what this browser had
 * seen BEFORE the page opened (the unread dots read against it), the eye
 * toggles (resolved, outdated), the `c` / `C` cursor and the toast.
 */
export function useReviewCommentState(runId: string) {
  const [drafts, setDrafts] = useState<ReviewDraft[]>(() => readDrafts(runId));
  const [editingKey, setEditingKey] = useState<string | null>(null);
  const [sendingKeys, setSendingKeys] = useState<ReadonlySet<string>>(() => new Set());
  const [startingManager, setStartingManager] = useState(false);
  const [toast, setToast] = useState<{ text: string; error?: boolean } | null>(null);
  const [commentCursor, setCommentCursor] = useState(-1);
  const toastTimer = useRef<number | undefined>(undefined);
  const [seen, setSeen] = useState<SeenMap>(() => readSeen(runId));
  const [showResolved, toggleShowResolved] = useStoredToggle(SHOW_RESOLVED_KEY);
  const [showOutdated, toggleShowOutdated] = useStoredToggle(SHOW_OUTDATED_KEY);
  const [deciding, setDeciding] = useState<ReadonlySet<string>>(() => new Set());

  const showToast = useCallback((text: string, error = false) => {
    setToast({ text, error });
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(null), error ? 6000 : 3600);
  }, []);
  useEffect(() => () => window.clearTimeout(toastTimer.current), []);

  const persistDrafts = useCallback(
    (next: ReviewDraft[]) => {
      setDrafts(next);
      writeDrafts(runId, next);
    },
    [runId],
  );

  return {
    drafts,
    persistDrafts,
    editingKey,
    setEditingKey,
    sendingKeys,
    setSendingKeys,
    startingManager,
    setStartingManager,
    toast,
    showToast,
    commentCursor,
    setCommentCursor,
    seen,
    setSeen,
    showResolved,
    toggleShowResolved,
    showOutdated,
    toggleShowOutdated,
    deciding,
    setDeciding,
  };
}

export type ReviewCommentState = ReturnType<typeof useReviewCommentState>;
