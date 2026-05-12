import type { PipelineListEntry, PipelineDetail, RunListEntry, RunState, PortDef, PortSide, FrontmatterFieldDecl } from "./types";

const BASE = "";

async function throwStructuredSaveError(resp: Response, fallback: string): Promise<never> {
  const body = await resp.json().catch(() => null);
  const err: Record<string, unknown> = {
    message: body?.message ?? body?.error ?? fallback,
    status: resp.status,
  };
  if (typeof body?.line === "number") err.line = body.line;
  throw err;
}

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

export interface MissingOutputsError {
  kind: "missing_outputs";
  missing: string[];
}

export interface MarkNodeDoneResult {
  ok: boolean;
  missingOutputs?: MissingOutputsError;
}

export async function markNodeDone(
  runId: string,
  nodeId: string,
  iter: number,
): Promise<MarkNodeDoneResult> {
  const resp = await fetch(
    `${BASE}/runs/${encodeURIComponent(runId)}/commands`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ kind: "mark_node_done", node_id: nodeId, iter }),
    },
  );
  if (resp.status === 409) {
    const body = await resp.json();
    return { ok: false, missingOutputs: { kind: "missing_outputs", missing: body.missing ?? [] } };
  }
  if (!resp.ok) throw new Error(`mark_node_done failed: ${resp.status}`);
  return { ok: true };
}

export async function attachSession(sessionId: string): Promise<void> {
  const resp = await fetch(
    `${BASE}/sessions/${encodeURIComponent(sessionId)}/attach`,
    { method: "POST" },
  );
  if (!resp.ok) throw new Error(`attach failed: ${resp.status}`);
}

export async function attachManager(runId: string): Promise<void> {
  const resp = await fetch(
    `${BASE}/sessions/${encodeURIComponent(runId)}/manager/attach`,
    { method: "POST" },
  );
  if (!resp.ok) throw new Error(`manager attach failed: ${resp.status}`);
}

export interface PaneResponse {
  content: string;
  session_name: string;
  resumed: boolean;
  stale: boolean;
}

export async function fetchPrompt(
  runId: string,
  nodeId: string,
  iter: number,
): Promise<string> {
  const resp = await fetch(
    `${BASE}/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/prompt?iter=${iter}`,
  );
  if (!resp.ok) throw new Error(`GET prompt failed: ${resp.status}`);
  return resp.text();
}

// --- Node IO ---

export interface FileInfo {
  path: string;
  exists: boolean;
  size: number | null;
  frontmatter: Record<string, unknown> | null;
}

export interface PortIO {
  port: string;
  repeated: boolean;
  files: FileInfo[];
}

export interface NodeIO {
  inputs: PortIO[];
  outputs: PortIO[];
}

export async function fetchNodeIO(
  runId: string,
  nodeId: string,
  iter: number,
): Promise<NodeIO> {
  const resp = await fetch(
    `${BASE}/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/io?iter=${iter}`,
  );
  if (!resp.ok) throw new Error(`GET io failed: ${resp.status}`);
  return resp.json();
}

export async function fetchArtifact(
  runId: string,
  relativePath: string,
): Promise<string> {
  const resp = await fetch(
    `${BASE}/runs/${encodeURIComponent(runId)}/artifact?path=${encodeURIComponent(relativePath)}`,
  );
  if (!resp.ok) throw new Error(`GET artifact failed: ${resp.status}`);
  return resp.text();
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
  pipeline_id?: string;
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

export async function forgetRun(runId: string): Promise<void> {
  const resp = await fetch(`${BASE}/runs/${encodeURIComponent(runId)}`, {
    method: "DELETE",
  });
  if (!resp.ok) throw new Error(`DELETE /runs/${runId} failed: ${resp.status}`);
}

// --- Run-scoped pipeline ---

export async function fetchRunPipeline(runId: string): Promise<PipelineDetail> {
  const resp = await fetch(`${BASE}/runs/${encodeURIComponent(runId)}/pipeline`);
  if (!resp.ok) throw new Error(`GET /runs/${runId}/pipeline failed: ${resp.status}`);
  return resp.json();
}

export async function saveRunPipeline(
  runId: string,
  yaml: string,
  prompts: Record<string, string>,
): Promise<void> {
  const resp = await fetch(`${BASE}/runs/${encodeURIComponent(runId)}/pipeline`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ yaml, prompts }),
  });
  if (!resp.ok) await throwStructuredSaveError(resp, `PUT /runs/${runId}/pipeline failed: ${resp.status}`);
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
  if (!resp.ok) await throwStructuredSaveError(resp, `PUT /pipelines/${id} failed: ${resp.status}`);
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

// --- Library API ---

export interface LibraryPort {
  name: string;
  repeated: boolean;
  side?: string;
  frontmatter?: Record<string, FrontmatterFieldDecl> | null;
  when?: Record<string, unknown> | null;
}

export function libraryPortToPortDef(port: LibraryPort, defaultSide: PortSide): PortDef {
  return {
    name: port.name,
    repeated: port.repeated,
    side: (port.side as PortSide) ?? defaultSide,
    ...(port.frontmatter ? { frontmatter: port.frontmatter } : {}),
    ...(port.when ? { when: port.when } : {}),
  };
}

export interface LibraryEntry {
  name: string;
  type: string;
  inputs: LibraryPort[];
  outputs: LibraryPort[];
  interactive: boolean;
  max_iter?: number | null;
  branches?: number | null;
  prompt: string;
}

export async function fetchLibrary(): Promise<LibraryEntry[]> {
  const resp = await fetch(`${BASE}/library`);
  if (!resp.ok) throw new Error(`GET /library failed: ${resp.status}`);
  return resp.json();
}

export interface LibrarySaveSpec {
  name: string;
  type: string;
  inputs: LibraryPort[];
  outputs: LibraryPort[];
  interactive: boolean;
  prompt: string;
}

export async function saveToLibrary(spec: LibrarySaveSpec): Promise<LibraryEntry> {
  const resp = await fetch(`${BASE}/library`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(spec),
  });
  if (!resp.ok) throw new Error(`POST /library failed: ${resp.status}`);
  return resp.json();
}

export async function deleteFromLibrary(name: string): Promise<void> {
  const resp = await fetch(`${BASE}/library/${encodeURIComponent(name)}`, {
    method: "DELETE",
  });
  if (!resp.ok) throw new Error(`DELETE /library/${name} failed: ${resp.status}`);
}

export interface InstantiateResult {
  spec: {
    name: string;
    type: string;
    inputs: LibraryPort[];
    outputs: LibraryPort[];
    interactive: boolean;
  };
  prompt: string;
}

export async function instantiateFromLibrary(name: string): Promise<InstantiateResult> {
  const resp = await fetch(`${BASE}/library/${encodeURIComponent(name)}/instantiate`, {
    method: "POST",
  });
  if (!resp.ok) throw new Error(`POST /library/${name}/instantiate failed: ${resp.status}`);
  return resp.json();
}

export interface DeletePipelineError {
  conflict: boolean;
  message: string;
}

export async function deletePipeline(id: string): Promise<void> {
  const resp = await fetch(`${BASE}/pipelines/${encodeURIComponent(id)}`, {
    method: "DELETE",
  });
  if (resp.status === 409) {
    const body = await resp.json();
    const err: DeletePipelineError = { conflict: true, message: body.error ?? "Pipeline has active runs" };
    throw err;
  }
  if (!resp.ok) throw new Error(`DELETE /pipelines/${id} failed: ${resp.status}`);
}

// --- Library Pipelines API ---

export type LibraryPipelineScope = "repo" | "user";

export interface LibraryPipelineEntry {
  id: string;
  name: string;
  scope: LibraryPipelineScope;
  node_count: number;
  modified: string | null;
  yaml: string;
}

export async function fetchLibraryPipelines(): Promise<LibraryPipelineEntry[]> {
  const resp = await fetch(`${BASE}/library/pipelines`);
  if (!resp.ok) throw new Error(`GET /library/pipelines failed: ${resp.status}`);
  return resp.json();
}

export interface SaveLibraryPipelineOptions {
  /// When set, save in-place at this id even if `name` changed. Required for
  /// rename-in-place: without it the daemon falls back to slug(name), which
  /// would orphan the previous entry.
  id?: string;
  scope?: LibraryPipelineScope;
}

export async function saveLibraryPipeline(
  name: string,
  yaml: string,
  prompts: Record<string, string> = {},
  options: SaveLibraryPipelineOptions = {},
): Promise<{ id: string; scope: LibraryPipelineScope }> {
  const resp = await fetch(`${BASE}/library/pipelines`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      name,
      yaml,
      prompts,
      ...(options.id ? { id: options.id } : {}),
      ...(options.scope ? { scope: options.scope } : {}),
    }),
  });
  if (!resp.ok) throw new Error(`POST /library/pipelines failed: ${resp.status}`);
  return resp.json();
}

export async function deleteLibraryPipeline(id: string): Promise<void> {
  const resp = await fetch(`${BASE}/library/pipelines/${encodeURIComponent(id)}`, {
    method: "DELETE",
  });
  if (!resp.ok) throw new Error(`DELETE /library/pipelines/${id} failed: ${resp.status}`);
}
