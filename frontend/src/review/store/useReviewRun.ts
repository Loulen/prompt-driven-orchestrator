import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { fetchRunRefs } from "../../run/api/artifacts";
import { fetchRun } from "../../run/api/runs";
import type { RunRefs } from "../../shared/generated/RunRefs";
import type { RunState } from "../../shared/generated/RunState";
import {
  defaultPair,
  deliverySignature,
  hasExplicitPair,
  pairFromSearch,
  reconcilePair,
  reviewUrl,
} from "../../run/model/runRefs";
import type { RefPair } from "../../run/model/runRefs";
import { allSeen, readSeen, writeSeen } from "../model/seen";

/**
 * The Run the Review page reads (#749): its state, its refs, and the pair of
 * refs on display — from the URL, else the daemon's default (#835: fork →
 * worktree while the Run's worktree exists, fork → tip after), validated once
 * the refs are known (a bad ref falls back with a notice), followed on
 * back/forward. Opening the page marks every reply of the Run seen for this
 * browser: that is what clears the Diff tab's unread badge.
 */
export function useReviewRun(runId: string) {
  const [run, setRunState] = useState<RunState | null>(null);
  const [runError, setRunError] = useState<string | null>(null);
  const [refs, setRefs] = useState<RunRefs | null>(null);
  // #835: until the refs are in, the URL pair is read against the fallback;
  // the diff waits for the refs so a URL with no pair loads the real default
  // once, not tip then worktree.
  const [refsSettled, setRefsSettled] = useState(false);
  const urlExplicit = useRef(hasExplicitPair(window.location.search));
  const [pair, setPairState] = useState<RefPair>(() => pairFromSearch(window.location.search));
  const [notice, setNotice] = useState<string | null>(null);
  const [reloadTick, setReloadTick] = useState(0);
  const openedSeenWritten = useRef(false);
  const runRef = useRef<RunState | null>(null);
  /** The delivery signature when the displayed diff was loaded — "tip moved" compares against it. */
  const sigAtLoad = useRef<string | null>(null);
  const pairRef = useRef(pair);
  useEffect(() => {
    pairRef.current = pair;
  }, [pair]);

  /** The Run as last fetched, for the render and for the callbacks. */
  const setRun = useCallback((r: RunState) => {
    runRef.current = r;
    setRunState(r);
  }, []);

  useEffect(() => {
    let stale = false;
    fetchRun(runId)
      .then((r) => {
        if (stale) return;
        setRun(r);
        // The signature at first sight, if the diff loaded before the Run did.
        if (sigAtLoad.current === null) sigAtLoad.current = deliverySignature(r.nodes);
      })
      .catch((e: unknown) => {
        if (!stale) setRunError(e instanceof Error ? e.message : String(e));
      });
    fetchRunRefs(runId)
      .then((r) => {
        if (stale) return;
        setRefs(r);
        // No pair in the URL: follow the daemon's default (#835).
        if (!urlExplicit.current) setPairState(defaultPair(r));
        setRefsSettled(true);
      })
      .catch(() => {
        // The pickers show ids; the diff loads with the URL pair anyway.
        if (!stale) setRefsSettled(true);
      });
    return () => {
      stale = true;
    };
  }, [runId, reloadTick, setRun]);

  useEffect(() => {
    if (run) document.title = `Review · ${run.pipeline_name} · ${runId}`;
  }, [run, runId]);

  // Opening the Review clears the Diff tab's unread badge (spec): every reply of
  // the Run counts as seen for this browser from now on. The `seen` snapshot the
  // dots read against stays what it was at open, so the page still points at
  // what is new.
  useEffect(() => {
    if (!run || openedSeenWritten.current) return;
    openedSeenWritten.current = true;
    writeSeen(runId, allSeen(run.review_comments, readSeen(runId)));
  }, [run, runId]);

  // Validate the URL pair once the refs are known; a bad ref falls back with a notice.
  const reconciledFor = useRef<RunRefs | null>(null);
  const defaults = useMemo(() => defaultPair(refs), [refs]);
  useEffect(() => {
    if (!refs || reconciledFor.current === refs) return;
    reconciledFor.current = refs;
    const { pair: next, notice: n } = reconcilePair(pairRef.current, refs);
    if (n) {
      setNotice(n);
      setPairState(next);
      window.history.replaceState(null, "", reviewUrl(runId, next, defaultPair(refs)));
    }
  }, [refs, runId]);

  /** The operator picked a pair: it becomes explicit, and the URL says it. */
  const choosePair = useCallback(
    (next: RefPair) => {
      urlExplicit.current = true;
      setPairState(next);
      setNotice(null);
      window.history.replaceState(null, "", reviewUrl(runId, next, defaults));
    },
    [runId, defaults],
  );
  useEffect(() => {
    const onPop = () => {
      urlExplicit.current = hasExplicitPair(window.location.search);
      setPairState(pairFromSearch(window.location.search, defaults));
    };
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, [defaults]);

  /** Fetch the Run again (after a gesture — the WebSocket refresh lands anyway). */
  const refreshRun = useCallback(async () => {
    try {
      setRun(await fetchRun(runId));
    } catch {
      // The WebSocket refresh lands anyway.
    }
  }, [runId, setRun]);

  return {
    run,
    runRef,
    setRun,
    refreshRun,
    runError,
    refs,
    refsSettled,
    pair,
    pairRef,
    choosePair,
    defaults,
    notice,
    setNotice,
    reloadTick,
    reload: () => setReloadTick((t) => t + 1),
    sigAtLoad,
  };
}
