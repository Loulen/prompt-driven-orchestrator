import type { PipelineListEntry, PipelineDetail, RunListEntry, RunState } from "./types";

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

export interface PaneResponse {
  content: string;
  session_name: string;
  resumed: boolean;
}

export async function fetchPane(
  runId: string,
  nodeId: string,
  iter: number,
): Promise<PaneResponse> {
  const resp = await fetch(
    `${BASE}/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/pane?iter=${iter}`,
  );
  if (!resp.ok) throw new Error(`GET pane failed: ${resp.status}`);
  return resp.json();
}

export async function fetchPipelines(): Promise<PipelineListEntry[]> {
  const resp = await fetch(`${BASE}/pipelines`);
  if (!resp.ok) throw new Error(`GET /pipelines failed: ${resp.status}`);
  return resp.json();
}

export interface CreateRunRequest {
  pipeline: string;
  input: string;
  variables: Record<string, unknown>;
}

export interface CreateRunResponse {
  run_id: string;
}

export async function createRun(req: CreateRunRequest): Promise<CreateRunResponse> {
  const resp = await fetch(`${BASE}/runs`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(req),
  });
  if (!resp.ok) throw new Error(`POST /runs failed: ${resp.status}`);
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

// --- Pipeline CRUD ---

export async function fetchPipeline(id: string): Promise<PipelineDetail> {
  const resp = await fetch(`${BASE}/pipelines/${encodeURIComponent(id)}`);
  if (!resp.ok) throw new Error(`GET /pipelines/${id} failed: ${resp.status}`);
  return resp.json();
}

export async function savePipeline(
  id: string,
  yaml: string,
  prompts: Record<string, string>,
): Promise<void> {
  const resp = await fetch(`${BASE}/pipelines/${encodeURIComponent(id)}`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ yaml, prompts }),
  });
  if (!resp.ok) throw new Error(`PUT /pipelines/${id} failed: ${resp.status}`);
}

export async function createPipeline(
  name: string,
  scope: string,
): Promise<{ id: string; scope: string; path: string }> {
  const resp = await fetch(`${BASE}/pipelines`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name, scope }),
  });
  if (!resp.ok) throw new Error(`POST /pipelines failed: ${resp.status}`);
  return resp.json();
}
