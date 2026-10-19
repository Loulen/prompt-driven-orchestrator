/**
 * The search over a Run's transcripts: every node iteration's entries holding
 * a query, by node then chronology, with an excerpt each.
 */

import type { RunTranscriptSearch } from "../../shared/generated/RunTranscriptSearch";
import { request } from "../../shared/http/client";

/** `client` names the search box asking: the daemon cancels its previous
 *  search when its next one arrives; `signal` drops the answer here. */
export function searchRunTranscripts(
  runId: string,
  query: string,
  client: string,
  signal?: AbortSignal,
): Promise<RunTranscriptSearch> {
  return request<RunTranscriptSearch>(
    "GET",
    `/runs/${encodeURIComponent(runId)}/transcripts/search`,
    { query: { q: query, client }, label: "GET transcript search", signal },
  );
}
