import { useEffect } from "react";
import { searchRunTranscripts } from "../api/transcriptSearch";
import { SEARCH_DEBOUNCE_MS } from "../model/runSearch";
import { useRunSearchStore } from "./runSearchStore";

/** This window's search box, as the daemon knows it: its next search cancels
 *  the previous one. */
const SEARCH_CLIENT = `search-${Math.random().toString(36).slice(2, 10)}`;

/**
 * Runs the open panel's search: `SEARCH_DEBOUNCE_MS` after the last keystroke,
 * the request before it aborted. The window's `client` id travels with each
 * request, so the daemon cancels the read of the previous one too.
 */
export function useRunSearch(): void {
  const runId = useRunSearchStore((s) => s.runId);
  const query = useRunSearchStore((s) => s.query);
  const settle = useRunSearchStore((s) => s.settle);

  useEffect(() => {
    if (!runId || !query.trim()) {
      settle({ status: "idle", result: null, error: null });
      return;
    }
    const controller = new AbortController();
    const timer = window.setTimeout(() => {
      useRunSearchStore.setState({ status: "searching" });
      searchRunTranscripts(runId, query.trim(), SEARCH_CLIENT, controller.signal)
        .then((result) => {
          if (!controller.signal.aborted) settle({ status: "done", result, error: null });
        })
        .catch((e: unknown) => {
          if (controller.signal.aborted) return;
          settle({
            status: "error",
            result: null,
            error: e instanceof Error ? e.message : "cannot search the transcripts",
          });
        });
    }, SEARCH_DEBOUNCE_MS);
    return () => {
      window.clearTimeout(timer);
      controller.abort();
    };
  }, [runId, query, settle]);
}
