import { useCallback, useEffect, useRef, useState } from "react";
import { fetchRun, fetchRuns } from "../api";
import { collectLiveSessions, isTerminalIterStatus, paneIterStatus } from "../lib/liveSessions";
import { pollAfterResponse } from "../lib/pollAfterResponse";
import {
  addPane,
  canAddPane,
  hasRestorable,
  loadLastMosaic,
  removePane,
  restoreMosaic,
  saveLastMosaic,
  type TerminalRef,
} from "../lib/terminalMosaic";

/**
 * How long after the previous answer an added node pane re-reads its
 * iteration's status (#1040). `GET /runs/{id}` is a full projection: never
 * stacked, never while the tab is hidden, never once the iteration is frozen.
 */
export const MOSAIC_STATUS_POLL_MS = 10_000;

/** A request to give the keyboard to a pane; `n` re-fires it for the same pane. */
export interface PaneFocusRequest {
  session: string;
  n: number;
}

export interface TerminalMosaic {
  /** The panes added around the primary, in order. Empty outside the expanded view. */
  extras: TerminalRef[];
  canAdd: boolean;
  /** Adds a pane, or focuses it when its session is already shown. */
  add: (ref: TerminalRef) => void;
  /** The ✕: out of the view only — no kill, no call. */
  remove: (session: string) => void;
  /** « Restore last mosaic » is on offer: a lone terminal, and a last mosaic to take back. */
  canRestore: boolean;
  restoring: boolean;
  /** What the last restore could not give back, if anything. */
  restoreNotice: string | null;
  restore: () => Promise<void>;
  focusRequest: PaneFocusRequest | null;
  /** The live status of each added node pane's iteration, by session. */
  statuses: Record<string, string | undefined>;
}

/**
 * #1040 — the Mosaïque of one expanded node terminal: what was added around it,
 * the poste's last mosaic, and the status of the added node panes.
 *
 * Closing the expanded view forgets the composition (the next one starts with
 * the node's terminal alone); the poste keeps it as its last mosaic.
 */
export function useTerminalMosaic(primary: TerminalRef, expanded: boolean): TerminalMosaic {
  const [extras, setExtras] = useState<TerminalRef[]>([]);
  const [focusRequest, setFocusRequest] = useState<PaneFocusRequest | null>(null);
  const [last, setLast] = useState<TerminalRef[] | null>(() => loadLastMosaic());
  const [restoring, setRestoring] = useState(false);
  const [restoreNotice, setRestoreNotice] = useState<string | null>(null);
  const [statuses, setStatuses] = useState<Record<string, string | undefined>>({});

  // Each opening reads the poste's last mosaic afresh: another node's view may
  // have written it since this one mounted.
  const [wasExpanded, setWasExpanded] = useState(expanded);
  if (wasExpanded !== expanded) {
    setWasExpanded(expanded);
    if (expanded) setLast(loadLastMosaic());
  }

  // Closing forgets the mosaic.
  if (!expanded && (extras.length > 0 || focusRequest !== null || restoreNotice !== null)) {
    setExtras([]);
    setFocusRequest(null);
    setRestoreNotice(null);
  }

  const remember = useCallback(
    (next: TerminalRef[]) => {
      const composition = [primary, ...next];
      if (composition.length < 2) return;
      saveLastMosaic(composition);
      setLast(composition);
    },
    [primary],
  );

  const focus = useCallback((session: string) => {
    setFocusRequest((r) => ({ session, n: (r?.n ?? 0) + 1 }));
  }, []);

  const add = useCallback(
    (ref: TerminalRef) => {
      const outcome = addPane(primary.session, extras, ref);
      if (outcome.added) {
        setExtras(outcome.extras);
        remember(outcome.extras);
      }
      if (outcome.focus) focus(outcome.focus);
    },
    [primary.session, extras, remember, focus],
  );

  const remove = useCallback(
    (session: string) => {
      const next = removePane(extras, session);
      setExtras(next);
      remember(next);
      focus(primary.session);
    },
    [extras, remember, focus, primary.session],
  );

  const restore = useCallback(async () => {
    setRestoring(true);
    setRestoreNotice(null);
    try {
      const live = await collectLiveSessions({ fetchRuns, fetchRun });
      const restored = restoreMosaic(primary.session, last, live);
      if (restored.length === 0) {
        setRestoreNotice("No terminal of the last mosaic is still live.");
        return;
      }
      setExtras(restored);
      remember(restored);
      focus(restored[restored.length - 1].session);
    } catch {
      setRestoreNotice("Could not list the live sessions.");
    } finally {
      setRestoring(false);
    }
  }, [primary.session, last, remember, focus]);

  // The added node panes follow their iteration's status, so one whose session
  // ends shows the frozen pane rather than a dropped connection. A frozen
  // iteration never changes again: its pane leaves the poll for good.
  const frozenSessions = useRef(new Set<string>());
  // The round in flight, if any, so a restarted poll (a pane added or removed)
  // waits for it rather than stacking a second request on the daemon.
  const roundInFlight = useRef<Promise<unknown>>(Promise.resolve());
  const watchKey = JSON.stringify(
    extras.flatMap((r) =>
      r.kind === "node" && r.paneSource
        ? [[r.session, r.runId, r.paneSource.nodeId, r.paneSource.iter] as const]
        : [],
    ),
  );
  useEffect(() => {
    const frozen = frozenSessions.current;
    let panes = (JSON.parse(watchKey) as [string, string, string, number][]).filter(
      ([session]) => !frozen.has(session),
    );
    if (panes.length === 0) return;
    let cancelled = false;
    const stop = pollAfterResponse(async () => {
      await roundInFlight.current;
      if (cancelled) return false;
      const runIds = [...new Set(panes.map(([, runId]) => runId))];
      const round = Promise.allSettled(runIds.map((id) => fetchRun(id)));
      roundInFlight.current = round;
      const runs = await round;
      if (cancelled) return false;
      const next: Record<string, string | undefined> = {};
      runs.forEach((res, i) => {
        if (res.status !== "fulfilled") return;
        for (const [session, runId, nodeId, iter] of panes) {
          if (runId !== runIds[i]) continue;
          next[session] = paneIterStatus(res.value, nodeId, iter);
          if (isTerminalIterStatus(next[session])) frozen.add(session);
        }
      });
      setStatuses((prev) => ({ ...prev, ...next }));
      panes = panes.filter(([session]) => !frozen.has(session));
      return panes.length > 0;
    }, MOSAIC_STATUS_POLL_MS);
    return () => {
      cancelled = true;
      stop();
    };
  }, [watchKey]);

  return {
    extras,
    canAdd: canAddPane(extras),
    add,
    remove,
    canRestore: expanded && extras.length === 0 && hasRestorable(primary.session, last),
    restoring,
    restoreNotice,
    restore,
    focusRequest,
    statuses,
  };
}
