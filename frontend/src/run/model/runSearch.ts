/**
 * The seam that opens the search over a Run's transcripts: the Transcript tab's
 * « Search run » button asks for it, the Run view answers by opening it.
 */

export const RUN_SEARCH_EVENT = "pdo:run-search";

export interface RunSearchRequest {
  runId: string;
}

/** Ask the Run view to open the search over `runId`'s transcripts. */
export function openRunSearch(runId: string): void {
  window.dispatchEvent(
    new CustomEvent<RunSearchRequest>(RUN_SEARCH_EVENT, { detail: { runId } }),
  );
}
