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
