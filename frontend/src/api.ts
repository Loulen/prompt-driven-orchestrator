import type { RunListEntry, RunState } from "./types";

const BASE = "";

export async function fetchRuns(): Promise<RunListEntry[]> {
  const resp = await fetch(`${BASE}/runs`);
  if (!resp.ok) throw new Error(`GET /runs failed: ${resp.status}`);
  return resp.json();
}

export async function fetchRun(runId: string): Promise<RunState> {
  const resp = await fetch(`${BASE}/runs/${encodeURIComponent(runId)}`);
  if (!resp.ok) throw new Error(`GET /runs/${runId} failed: ${resp.status}`);
  return resp.json();
}

export async function fetchRunEvents(runId: string): Promise<unknown[]> {
  const resp = await fetch(`${BASE}/runs/${encodeURIComponent(runId)}/events`);
  if (!resp.ok) throw new Error(`GET /runs/${runId}/events failed: ${resp.status}`);
  return resp.json();
}

export async function cleanupRun(runId: string): Promise<void> {
  const resp = await fetch(`${BASE}/runs/${encodeURIComponent(runId)}/commands`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ kind: "cleanup_run" }),
  });
  if (!resp.ok) throw new Error(`POST /runs/${runId}/commands failed: ${resp.status}`);
}
