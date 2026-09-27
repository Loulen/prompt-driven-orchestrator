/**
 * The Runs as the run context serves them: the list, a Run's detail and its
 * event log, its drift from its source, and forgetting it.
 */

import type { RunState } from "../../shared/generated/RunState";
import type { RunListEntry } from "../../shared/generated/RunListEntry";
import type { DaemonEvent } from "../../shared/generated/DaemonEvent";
import type { SourceDrift } from "../../shared/generated/SourceDrift";
import { request } from "../../shared/http/client";

/** A Run's detail: its projection, filled with what the daemon reads live. */
export function fetchRun(runId: string): Promise<RunState> {
  return request<RunState>("GET", `/runs/${encodeURIComponent(runId)}`);
}

/** Every event of a Run's log, in append order (`[]` for an unknown Run). */
export function fetchRunEvents(runId: string): Promise<DaemonEvent[]> {
  return request<DaemonEvent[]>("GET", `/runs/${encodeURIComponent(runId)}/events`);
}

export function fetchRuns(): Promise<RunListEntry[]> {
  return request<RunListEntry[]>("GET", "/runs");
}

export function forgetRun(runId: string): Promise<void> {
  return request<void>(
    "DELETE",
    `/runs/${encodeURIComponent(runId)}`,
    { responseMode: "void", label: `DELETE /runs/${runId}` },
  );
}

/**
 * A Run's drift from its source branch since the fork (#803, ADR-0070 §4).
 *
 * A pure READ over local refs — it never fetches, because opening a Run must not
 * put traffic on someone's repository. `unavailable` is a 200, not an error: an
 * archived Run whose branch was cleaned up is the expected end of a Run's life.
 */
export function fetchSourceDrift(runId: string): Promise<SourceDrift> {
  return request<SourceDrift>(
    "GET",
    `/runs/${encodeURIComponent(runId)}/source-drift`,
    { label: `GET /runs/${runId}/source-drift` },
  );
}
