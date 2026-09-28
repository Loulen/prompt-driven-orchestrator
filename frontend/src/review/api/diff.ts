import type { StructuredDiff } from "../../shared/generated/StructuredDiff";
import { request } from "../../shared/http/client";

export function fetchRunDiff(runId: string): Promise<string> {
  return request<string>(
    "GET",
    `/runs/${encodeURIComponent(runId)}/diff`,
    { responseMode: "text", label: `GET /runs/${runId}/diff` },
  );
}

/**
 * Structured Run diff (#748): files → hunks → lines, computed by the daemon in
 * the Run's effective repository. No `from`/`to` = fork point → Run tip, the
 * exact bounds of `run.loc`. An explicit pair compares two Run refs.
 */
export function fetchRunStructuredDiff(
  runId: string,
  refs?: { from?: string; to?: string },
): Promise<StructuredDiff> {
  const params = new URLSearchParams();
  if (refs?.from) params.set("from", refs.from);
  if (refs?.to) params.set("to", refs.to);
  const qs = params.size > 0 ? `?${params.toString()}` : "";
  return request<StructuredDiff>(
    "GET",
    `/runs/${encodeURIComponent(runId)}/diff/structured${qs}`,
    { label: `GET /runs/${runId}/diff/structured` },
  );
}
