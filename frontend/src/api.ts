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

export async function markNodeDone(
  runId: string,
  nodeId: string,
  iter: number,
): Promise<void> {
  const resp = await fetch(
    `${BASE}/runs/${encodeURIComponent(runId)}/commands`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ kind: "mark_node_done", node_id: nodeId, iter }),
    },
  );
  if (!resp.ok) throw new Error(`mark_node_done failed: ${resp.status}`);
}

export async function attachSession(sessionId: string): Promise<void> {
  const resp = await fetch(
    `${BASE}/sessions/${encodeURIComponent(sessionId)}/attach`,
    { method: "POST" },
  );
  if (!resp.ok) throw new Error(`attach failed: ${resp.status}`);
}

export async function cleanupRun(runId: string): Promise<void> {
  const resp = await fetch(`${BASE}/runs/${encodeURIComponent(runId)}/commands`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ kind: "cleanup_run" }),
  });
  if (!resp.ok) throw new Error(`POST /runs/${runId}/commands failed: ${resp.status}`);

}
