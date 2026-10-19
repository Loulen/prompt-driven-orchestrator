import { useCallback, useEffect, useRef, useState } from "react";
import { fetchTranscriptPage } from "../api/transcript";
import {
  EMPTY_TRANSCRIPT,
  mergeCounts,
  mergePage,
  refreshFrom,
  remaining,
  type TranscriptView,
} from "../model/transcript";

/** Entries per page. */
export const TRANSCRIPT_PAGE = 50;
/** How often a live iteration's transcript is re-read, in ms. */
export const TRANSCRIPT_POLL_MS = 2000;

/**
 * A node iteration's transcript, loaded page by page: the first page on mount,
 * the next one on `loadMore`, and — while the iteration is `live` — what the
 * session appends (and the results of the tool calls still running), never the
 * whole transcript again. Past the loaded entries, a live read only updates
 * the counts: the reader loads the rest by scrolling.
 *
 * One hook per node iteration: its caller remounts it (a `key`) when the node
 * or the iteration changes.
 */
export function useTranscript(
  runId: string,
  nodeId: string,
  iter: number,
  live: boolean,
) {
  const [view, setView] = useState<TranscriptView>(EMPTY_TRANSCRIPT);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const viewRef = useRef(view);
  useEffect(() => {
    viewRef.current = view;
  }, [view]);
  const busy = useRef(false);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const read = useCallback(
    async (after: number, countsOnly = false) => {
      if (busy.current) return;
      busy.current = true;
      try {
        const page = await fetchTranscriptPage(
          runId,
          nodeId,
          iter,
          after,
          countsOnly ? 1 : TRANSCRIPT_PAGE,
        );
        if (!mounted.current) return;
        setView((v) => {
          const next = countsOnly ? mergeCounts(v, page) : mergePage(v, after, page);
          viewRef.current = next;
          return next;
        });
        setLoaded(true);
        setError(null);
      } catch (e) {
        if (!mounted.current) return;
        setError(e instanceof Error ? e.message : "cannot read the transcript");
        setLoaded(true);
      } finally {
        busy.current = false;
      }
    },
    [runId, nodeId, iter],
  );

  useEffect(() => {
    void read(0);
  }, [read]);

  const refresh = useCallback(() => {
    const v = viewRef.current;
    const from = refreshFrom(v, TRANSCRIPT_PAGE);
    if (from >= v.entries.length && remaining(v) > 0) void read(v.total, true);
    else void read(from);
  }, [read]);

  // Live: re-read on a cadence. An iteration that settles under the reader's
  // eyes gets one last read, for what landed as it ended.
  const wasLive = useRef(live);
  useEffect(() => {
    const settled = wasLive.current && !live;
    wasLive.current = live;
    if (settled) refresh();
    if (!live) return;
    const timer = window.setInterval(refresh, TRANSCRIPT_POLL_MS);
    return () => window.clearInterval(timer);
  }, [live, refresh]);

  const loadMore = useCallback(() => {
    const v = viewRef.current;
    if (remaining(v) > 0) void read(v.entries.length);
  }, [read]);

  return { view, loaded, error, loadMore };
}
