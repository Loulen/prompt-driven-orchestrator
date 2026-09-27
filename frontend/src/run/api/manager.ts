/**
 * The Run's Pipeline Manager on demand: start it, stop it.
 */

import { request } from "../../shared/http/client";

/** Manager on demand: start the Run's Pipeline Manager session on demand.
 *  Idempotent — the daemon answers `created: false` when the session already
 *  existed (a double-click Start is a benign re-answer). */
export function startRunManager(runId: string): Promise<{ ok: boolean; session: string; created: boolean }> {
  return request(
    "POST",
    `/runs/${encodeURIComponent(runId)}/manager/start`,
    { label: `POST /runs/${runId}/manager/start` },
  );
}

/** Manager on demand: stop the Run's Pipeline Manager session. A stop on a
 *  session that is already gone is a calm no-op (`stopped: false`). */
export function stopRunManager(runId: string): Promise<{ ok: boolean; stopped: boolean }> {
  return request(
    "POST",
    `/runs/${encodeURIComponent(runId)}/manager/stop`,
    { label: `POST /runs/${runId}/manager/stop` },
  );
}
