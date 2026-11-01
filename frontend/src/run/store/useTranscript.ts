import { useCallback, useEffect, useRef, useState } from "react";
import { fetchTranscriptPage } from "../api/transcript";
import type { TranscriptOrder } from "../../lib/uiPrefs";
import {
  EMPTY_TRANSCRIPT,
  loadedEnd,
  mergeCounts,
  mergePage,
  newerRemaining,
  olderPage,
  refreshFrom,
  type TranscriptView,
} from "../model/transcript";

/** Entries per page. */
export const TRANSCRIPT_PAGE = 50;
/** How often a live iteration's transcript is re-read, in ms. */
export const TRANSCRIPT_POLL_MS = 2000;

/** What a read folds into the view: its entries, only its counts, or — the
 *  first read of a view shown newest first — its entries when they reach the
 *  end, else only the counts (the tail is read next). A whole page rather
 *  than one entry: the transcripts that fit in one page, most of them, take a
 *  single read. */
type ReadMode = "page" | "counts" | "head";

/**
 * A node iteration's transcript, loaded page by page, never whole:
 *
 * - oldest first: the first page on mount, the next one on `loadMore`;
 * - newest first: the first read learns the counts (and is the whole
 *   transcript when it fits one page), then the last page, then the older
 *   ones on `loadMore`.
 *
 * While the iteration is `live`, it reads what the session appends (and the
 * results of the tool calls still running), never the whole transcript again.
 * Past the loaded entries, a live read only updates the counts: the reader
 * loads the rest by scrolling.
 *
 * One hook per node iteration and order: its caller remounts it (a `key`)
 * when the node, the iteration or the order changes.
 */
export function useTranscript(
  runId: string,
  nodeId: string,
  iter: number,
  live: boolean,
  order: TranscriptOrder = "oldest-first",
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
    async (after: number, limit: number, mode: ReadMode = "page") => {
      if (busy.current) return;
      busy.current = true;
      try {
        const page = await fetchTranscriptPage(runId, nodeId, iter, after, limit);
        if (!mounted.current) return;
        setView((v) => {
          const whole = mode === "page" || (mode === "head" && page.next == null);
          const next = whole ? mergePage(v, after, page) : mergeCounts(v, page);
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
    void read(0, TRANSCRIPT_PAGE, order === "newest-first" ? "head" : "page");
  }, [read, order]);

  const loadOlder = useCallback(() => {
    const at = olderPage(viewRef.current, TRANSCRIPT_PAGE);
    if (at) void read(at.after, at.limit);
  }, [read]);

  // Newest first: a transcript longer than one page shows its tail first —
  // read as soon as the counts are known (and again if a live read took the
  // turn).
  useEffect(() => {
    if (order === "newest-first" && loaded && !error && view.entries.length === 0 && view.total > 0) loadOlder();
  }, [order, loaded, error, view, loadOlder]);

  const refresh = useCallback(() => {
    const v = viewRef.current;
    if (order === "newest-first" && v.entries.length === 0) {
      void read(0, 1, "counts");
      return;
    }
    const from = refreshFrom(v, TRANSCRIPT_PAGE);
    // Newest first, the newest end is always loaded: read on from it, a page
    // per beat when the session writes faster than that.
    if (order === "oldest-first" && from >= loadedEnd(v) && newerRemaining(v) > 0) void read(v.total, 1, "counts");
    else void read(from, TRANSCRIPT_PAGE);
  }, [read, order]);

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

  const loadNewer = useCallback(() => {
    const v = viewRef.current;
    if (newerRemaining(v) > 0) void read(loadedEnd(v), TRANSCRIPT_PAGE);
  }, [read]);

  /** The next page away from the end the tab opened on. */
  const loadMore = order === "newest-first" ? loadOlder : loadNewer;

  return { view, loaded, error, loadMore, loadOlder, loadNewer };
}
