/**
 * The canvas toolbar's Run actions: the Review quick access, and on a finished Run reopen, retry-all and the shell.
 */

import { useCallback, useState } from "react";
import type { RunState } from "../../shared/generated/RunState";
import { isTerminalRun } from "../../run/model/runStatus";
import { pendingCount, pendingTone, reviewQuickAccessTitle } from "../../lib/reviewComments";
import { reviewUrl } from "../../lib/runRefs";
import { useReviewSeen } from "../../hooks/useReviewUnread";
import { reopenRun, retryAll } from "../../api";
import { useRunShell } from "../../session/store/useRunShell";
// #723 — orchestrator pastilles on the run-view node card.

export function useCanvasRunActions(activeRunState: RunState | null, onSelectRun?: (runId: string) => void) {
  // #752 (CONTEXT.md « Accès rapide Review »): the toolbar's Review quick access
  // — a link to the Run's Review page with the pending-comment pill — on every
  // non-archived Run (an archived Run has no diff to review). The tone follows
  // the same "seen" marker as the Diff tab's unread pill. Repositories moved to
  // a tab of the Run panel, so the old sidebar toggle is gone.
  const reviewSeen = useReviewSeen(activeRunState ?? null);
  const reviewHref =
    activeRunState != null && activeRunState.status !== "archived"
      ? reviewUrl(activeRunState.run_id)
      : undefined;
  const reviewPending = pendingCount(activeRunState?.review_comments);
  const reviewTone = pendingTone(activeRunState?.review_comments, reviewSeen);
  const reviewTitle = reviewQuickAccessTitle(activeRunState?.review_comments, reviewSeen);

  // #598 / ADR-0049: the finished-run action group is contextual to a TERMINAL,
  // non-archived run (`isTerminalRun` INCLUDES `archived`, so exclude it — an
  // archived run has no worktree to reopen or shell into). Absent on a live run.
  const finishedRun =
    activeRunState != null &&
    isTerminalRun(activeRunState.status) &&
    activeRunState.status !== "archived";

  // Ad-hoc bash shell opened on the terminal run from the toolbar (#316/#598).
  const runShell = useRunShell();
  // Retry-all confirm gate (destructive: archive + fresh run).
  const [confirmRetryAll, setConfirmRetryAll] = useState(false);

  const handleReopen = useCallback(async () => {
    if (!activeRunState) return;
    // Optimistic: the daemon re-projects to `running` and broadcasts events; the
    // App's SSE subscription refreshes the run state, so the group disappears on
    // its own. No modal (contrast with Retry-all) — one click, one round-trip.
    try {
      await reopenRun(activeRunState.run_id);
    } catch {
      // The daemon gate may 409 (e.g. a live run raced terminal); nothing
      // actionable here — the run state refresh will reconcile the UI.
    }
  }, [activeRunState]);

  const handleRetryAll = useCallback(async () => {
    if (!activeRunState) return;
    setConfirmRetryAll(false);
    try {
      const { run_id } = await retryAll(activeRunState.run_id);
      onSelectRun?.(run_id);
    } catch {
      // Silent — mirrors the left-panel Retry-all.
    }
  }, [activeRunState, onSelectRun]);

  const { open: openShell } = runShell;
  const handleOpenShell = useCallback(async () => {
    if (activeRunState) await openShell(activeRunState.run_id);
  }, [activeRunState, openShell]);
  return { reviewHref, reviewPending, reviewTone, reviewTitle, finishedRun, runShell, confirmRetryAll, setConfirmRetryAll, handleReopen, handleRetryAll, handleOpenShell };
}
