import { useCallback, useEffect, useState } from "react";
import { fetchRemotes, fetchSourceDrift } from "../api";
import type { BranchFetchError, RunState, SourceDrift } from "../types";

/**
 * A Run's **dérive de la source**, and the one gesture that refreshes its remote
 * half (#803, ADR-0070 §4; CONTEXT.md § « Dérive de la source »).
 *
 * Three rules, all of them decisions rather than conveniences:
 *
 * 1. **Recomputed on display, never polled.** The drift is a measurement over local
 *    refs; it changes when the Run commits or when someone pulls, and neither is an
 *    event worth a timer. Mounting the panel reads it once — that is what "recalculée
 *    à chaque affichage" means.
 * 2. **No implicit fetch, ever.** Opening a Run must not put traffic on someone's
 *    repository (ADR-0070 §1). `refresh` is wired to a button and nothing else.
 * 3. **A failed fetch degrades one axis, blocks nothing.** The Run's own commits are
 *    still counted (they live on a local branch no remote has an opinion about); only
 *    the `m` side becomes unknown, dated from the last successful fetch.
 *
 * Shared by the Info tab's Source block and the Repositories tab's primary row, so
 * the two can never show different numbers for the same Run.
 */

/**
 * The answer AND the Run it is about, in one value.
 *
 * Keeping the two together is what makes switching Runs safe with no effect that
 * blanks the state first: the reading below simply does not match, so the previous
 * Run's numbers are never on screen for even one frame under a name that is not
 * theirs — the #454 staleness, in Run shape. The fetch verdict travels in the same
 * value for the same reason: new numbers beside a stale "the fetch failed" (or the
 * reverse) would be two halves of two different moments.
 */
interface Answer {
  runId: string | null;
  drift: SourceDrift | null;
  fetchError: BranchFetchError | null;
}

const NOTHING: Answer = { runId: null, drift: null, fetchError: null };

export function useSourceDrift(run: RunState | null) {
  const [answer, setAnswer] = useState<Answer>(NOTHING);
  const [fetchingRun, setFetchingRun] = useState<string | null>(null);

  const runId = run?.run_id ?? null;
  const repo = run?.target_repo ?? null;

  useEffect(() => {
    if (!runId) return;
    let cancelled = false;
    fetchSourceDrift(runId)
      .then((drift) => {
        if (!cancelled) setAnswer({ runId, drift, fetchError: null });
      })
      .catch(() => {
        // A drift nobody could read is simply not shown: the chip disappears rather
        // than claiming a zero. Never an error banner — this is an ornament on a
        // panel, not the panel.
        if (!cancelled) setAnswer({ runId, drift: null, fetchError: null });
      });
    return () => {
      cancelled = true;
    };
  }, [runId]);

  /**
   * The Run view's only gesture: fetch every remote of the Run's repository (the
   * #802 verb, unchanged — a fetch run from here refreshes exactly what the next
   * launch form will read, because it is the same repository), then recompute.
   */
  const refresh = useCallback(async () => {
    if (!runId || !repo || fetchingRun) return;
    setFetchingRun(runId);
    // A failed fetch is a verdict to carry, never a reason to stop: the drift is a
    // local measurement and is recomputed either way (ADR-0070 §1).
    let fetchError: BranchFetchError | null;
    try {
      fetchError = (await fetchRemotes(repo)).fetch_error;
    } catch (e) {
      fetchError = { kind: "failed", message: e instanceof Error ? e.message : String(e) };
    }
    try {
      const drift = await fetchSourceDrift(runId);
      setAnswer({ runId, drift, fetchError });
    } catch {
      setAnswer({ runId, drift: null, fetchError });
    } finally {
      setFetchingRun((f) => (f === runId ? null : f));
    }
  }, [runId, repo, fetchingRun]);

  // Derived, not stored: a stale answer is one that names another Run.
  const current = answer.runId === runId ? answer : NOTHING;
  return {
    drift: current.drift,
    fetchError: current.fetchError,
    fetching: fetchingRun != null && fetchingRun === runId,
    refresh,
  };
}
