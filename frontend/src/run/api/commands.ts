/**
 * The Pipeline Manager's commands on a Run (`POST /runs/{id}/commands`), as
 * the UI sends them: on a node (kill, restart), on the Run (pause, resume,
 * reopen, rename, archive, retry all), on a loop region (end, bump).
 */

import type { CreateRunResponse } from "../../api";
import { request } from "../../shared/http/client";

export function killNode(
  runId: string,
  nodeId: string,
  iter: number,
): Promise<void> {
  return request<void>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/commands`,
    { body: { kind: "kill_node", node_id: nodeId, iter }, responseMode: "void", label: "kill_node" },
  );
}

export function restartNode(
  runId: string,
  nodeId: string,
  iter: number,
): Promise<void> {
  return request<void>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/commands`,
    { body: { kind: "restart_node", node_id: nodeId, iter }, responseMode: "void", label: "restart_node" },
  );
}

export function pauseRun(runId: string): Promise<void> {
  return request<void>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/commands`,
    { body: { kind: "pause_run" }, responseMode: "void", label: "pause_run" },
  );
}

export function resumeRun(runId: string): Promise<void> {
  return request<void>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/commands`,
    { body: { kind: "resume_run" }, responseMode: "void", label: "resume_run" },
  );
}

/**
 * The global re-open (#598 / ADR-0049): "re-project + drive the new". Surfaced by
 * the Play button in the run-level toolbar. Lifts a terminal (or incident-parked)
 * run back to `running` by a safe re-projection — satisfied `(node, iter)` are
 * frozen (never re-spawned, anti-#221), only the unsatisfied work runs. Distinct
 * from `retryAll` (which archives and forks a NEW run with a different id).
 */
export function reopenRun(runId: string): Promise<void> {
  return request<void>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/commands`,
    { body: { kind: "reopen_run" }, responseMode: "void", label: "reopen_run" },
  );
}

/**
 * Route a loop region by id from the Pipeline Manager (ADR-0011 / #152): end it
 * (fire its completion) so a region blocked "exhausted — unrouted" leaves the
 * region and the run proceeds. The daemon resumes the run as part of the command.
 */
export function endRegion(runId: string, regionId: string): Promise<void> {
  return request<void>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/commands`,
    { body: { kind: "end_region", region_id: regionId }, responseMode: "void", label: "end_region" },
  );
}

/**
 * Route a loop region by id from the Pipeline Manager (ADR-0011 / #152): bump it
 * (run `additionalIter` more iterations) so a region blocked "exhausted —
 * unrouted" resumes iterating. The daemon resumes the run as part of the command.
 */
export function bumpRegion(
  runId: string,
  regionId: string,
  additionalIter: number,
): Promise<void> {
  return request<void>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/commands`,
    {
      body: { kind: "bump_region", region_id: regionId, additional_iter: additionalIter },
      responseMode: "void",
      label: "bump_region",
    },
  );
}

export function retryAll(runId: string): Promise<CreateRunResponse> {
  return request<CreateRunResponse>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/commands`,
    { body: { kind: "retry_all" }, label: "retry_all" },
  );
}

export function cleanupRun(runId: string): Promise<void> {
  return request<void>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/commands`,
    { body: { kind: "cleanup_run" }, responseMode: "void", label: `POST /runs/${runId}/commands` },
  );
}

export function renameRun(runId: string, name: string): Promise<void> {
  return request<void>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/commands`,
    { body: { kind: "rename_run", name }, responseMode: "void", label: `POST /runs/${runId}/commands` },
  );
}
