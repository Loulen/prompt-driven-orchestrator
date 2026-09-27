/**
 * A Run as the run context serves it: its detail and its event log.
 */

import type { RunState } from "../../shared/generated/RunState";
import type { DaemonEvent } from "../../shared/generated/DaemonEvent";
import { request } from "../../shared/http/client";

/** A Run's detail: its projection, filled with what the daemon reads live. */
export function fetchRun(runId: string): Promise<RunState> {
  return request<RunState>("GET", `/runs/${encodeURIComponent(runId)}`);
}

/** Every event of a Run's log, in append order (`[]` for an unknown Run). */
export function fetchRunEvents(runId: string): Promise<DaemonEvent[]> {
  return request<DaemonEvent[]>("GET", `/runs/${encodeURIComponent(runId)}/events`);
}
