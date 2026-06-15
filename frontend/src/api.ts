import type { PipelineListEntry, PipelineDetail, PipelineDef, RunListEntry, RunState, PortDef, PortSide, PortType, FrontmatterFieldDecl, Trigger, TriggerFire, DaemonStatus } from "./types";

const BASE = "";

async function throwStructuredSaveError(resp: Response, fallback: string): Promise<never> {
  const body = await resp.json().catch(() => null);
  let message: string = body?.message ?? body?.error ?? fallback;
  // A mid-run mutation rejection (409, ADR-0007 / #211) carries the "why" in
  // `rejections[].reason` — surface it, not just "mutation rejected".
  if (Array.isArray(body?.rejections)) {
    const reasons = body.rejections
      .map((r: { reason?: string }) => r?.reason)
      .filter((r: unknown): r is string => typeof r === "string");
    if (reasons.length > 0) message = `${message}: ${reasons.join("; ")}`;
  }
  const err: Record<string, unknown> = {
    message,
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

export async function fetchSessions(): Promise<DaemonStatus> {
  const resp = await fetch(`${BASE}/sessions`);
  if (!resp.ok) throw new Error(`GET /sessions failed: ${resp.status}`);
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
  /**
   * Provenance of `content` (#205): "live" (captured from a running session),
   * "resumed" (a dead latest-iter session was re-attached), "snapshot" (the
   * persisted post-mortem pane of a reaped terminal node), or "unavailable"
   * (no session and no snapshot).
   */
  source: "live" | "resumed" | "snapshot" | "unavailable";
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
  port_type?: PortType;
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

export function artifactUrl(runId: string, relativePath: string): string {
  return `${BASE}/runs/${encodeURIComponent(runId)}/artifact?path=${encodeURIComponent(relativePath)}`;
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
  target_repo?: string;
  source_branch?: string;
  name?: string;
  images?: File[];
}

export interface CreateRunResponse {
  run_id: string;
}

export async function createRun(req: CreateRunRequest): Promise<CreateRunResponse> {
  const hasImages = req.images && req.images.length > 0;

  let resp: Response;
  if (hasImages) {
    const form = new FormData();
    form.append("pipeline", req.pipeline);
    form.append("input", req.input);
    form.append("variables", JSON.stringify(req.variables));
    if (req.pipeline_id) form.append("pipeline_id", req.pipeline_id);
    if (req.target_repo) form.append("target_repo", req.target_repo);
    if (req.source_branch) form.append("source_branch", req.source_branch);
    if (req.name) form.append("name", req.name);
    for (const file of req.images!) {
      form.append("images", file, file.name);
    }
    resp = await fetch(`${BASE}/runs`, { method: "POST", body: form });
  } else {
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    const { images: _omitted, ...jsonBody } = req;
    resp = await fetch(`${BASE}/runs`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(jsonBody),
    });
  }

  if (!resp.ok) {
    const body = await resp.json().catch(() => null);
    throw new Error(body?.error ?? `POST /runs failed: ${resp.status}`);
  }
  return resp.json();
}

// --- Triggers (#160) ---

export interface CreateTriggerRequest {
  name: string;
  pipeline_id: string;
  cron: string;
  input_template?: string;
  target_repo?: string;
  source_branch?: string;
  variables?: Record<string, unknown>;
  guard_command?: string;
  overlap_policy?: string;
}

export async function fetchTriggers(): Promise<Trigger[]> {
  const resp = await fetch(`${BASE}/triggers`);
  if (!resp.ok) throw new Error(`GET /triggers failed: ${resp.status}`);
  return resp.json();
}

export async function createTrigger(req: CreateTriggerRequest): Promise<Trigger> {
  const resp = await fetch(`${BASE}/triggers`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(req),
  });
  if (!resp.ok) {
    const body = await resp.json().catch(() => null);
    throw new Error(body?.error ?? `POST /triggers failed: ${resp.status}`);
  }
  return resp.json();
}

export async function fetchTrigger(triggerId: string): Promise<Trigger> {
  const resp = await fetch(`${BASE}/triggers/${encodeURIComponent(triggerId)}`);
  if (!resp.ok) throw new Error(`GET /triggers/${triggerId} failed: ${resp.status}`);
  return resp.json();
}

/**
 * A partial Trigger edit (#162). Omitted fields are left unchanged. `enabled`
 * toggles activation; the config fields cover schedule, input template, and
 * overlap policy (plus name/repo/branch/guard for completeness).
 */
export interface UpdateTriggerRequest {
  name?: string;
  enabled?: boolean;
  cron?: string;
  input_template?: string;
  overlap_policy?: string;
  target_repo?: string | null;
  source_branch?: string | null;
  guard_command?: string | null;
  variables?: Record<string, unknown>;
}

export async function updateTrigger(
  triggerId: string,
  req: UpdateTriggerRequest,
): Promise<Trigger> {
  const resp = await fetch(`${BASE}/triggers/${encodeURIComponent(triggerId)}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(req),
  });
  if (!resp.ok) {
    const body = await resp.json().catch(() => null);
    throw new Error(body?.error ?? `PATCH /triggers/${triggerId} failed: ${resp.status}`);
  }
  return resp.json();
}

export async function deleteTrigger(triggerId: string): Promise<void> {
  const resp = await fetch(`${BASE}/triggers/${encodeURIComponent(triggerId)}`, {
    method: "DELETE",
  });
  if (!resp.ok && resp.status !== 404) {
    throw new Error(`DELETE /triggers/${triggerId} failed: ${resp.status}`);
  }
}

export async function fetchTriggerFires(triggerId: string): Promise<TriggerFire[]> {
  const resp = await fetch(`${BASE}/triggers/${encodeURIComponent(triggerId)}/fires`);
  if (!resp.ok) throw new Error(`GET /triggers/${triggerId}/fires failed: ${resp.status}`);
  return resp.json();
}

// --- Repo validation and branch listing ---

export interface ValidateRepoResponse {
  valid: boolean;
  error?: string;
}

export async function validateRepo(path: string): Promise<ValidateRepoResponse> {
  const resp = await fetch(`${BASE}/repos/validate?path=${encodeURIComponent(path)}`);
  return resp.json();
}

export async function listBranches(repoPath: string): Promise<string[]> {
  const resp = await fetch(`${BASE}/repos/branches?path=${encodeURIComponent(repoPath)}`);
  if (!resp.ok) throw new Error(`GET /repos/branches failed: ${resp.status}`);
  return resp.json();
}

export async function fetchRecentRepos(): Promise<string[]> {
  const resp = await fetch(`${BASE}/repos/recent`);
  if (!resp.ok) throw new Error(`GET /repos/recent failed: ${resp.status}`);
  return resp.json();
}

export async function killNode(
  runId: string,
  nodeId: string,
  iter: number,
): Promise<void> {
  const resp = await fetch(
    `${BASE}/runs/${encodeURIComponent(runId)}/commands`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ kind: "kill_node", node_id: nodeId, iter }),
    },
  );
  if (!resp.ok) throw new Error(`kill_node failed: ${resp.status}`);
}

export async function restartNode(
  runId: string,
  nodeId: string,
  iter: number,
): Promise<void> {
  const resp = await fetch(
    `${BASE}/runs/${encodeURIComponent(runId)}/commands`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ kind: "restart_node", node_id: nodeId, iter }),
    },
  );
  if (!resp.ok) throw new Error(`restart_node failed: ${resp.status}`);
}

export async function pauseRun(runId: string): Promise<void> {
  const resp = await fetch(`${BASE}/runs/${encodeURIComponent(runId)}/commands`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ kind: "pause_run" }),
  });
  if (!resp.ok) throw new Error(`pause_run failed: ${resp.status}`);
}

export async function resumeRun(runId: string): Promise<void> {
  const resp = await fetch(`${BASE}/runs/${encodeURIComponent(runId)}/commands`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ kind: "resume_run" }),
  });
  if (!resp.ok) throw new Error(`resume_run failed: ${resp.status}`);
}

/**
 * Route a loop region by id from the Pipeline Manager (ADR-0011 / #152): end it
 * (fire its completion) so a region blocked "exhausted — unrouted" leaves the
 * region and the run proceeds. The daemon resumes the run as part of the command.
 */
export async function endRegion(runId: string, regionId: string): Promise<void> {
  const resp = await fetch(`${BASE}/runs/${encodeURIComponent(runId)}/commands`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ kind: "end_region", region_id: regionId }),
  });
  if (!resp.ok) throw new Error(`end_region failed: ${resp.status}`);
}

/**
 * Route a loop region by id from the Pipeline Manager (ADR-0011 / #152): bump it
 * (run `additionalIter` more iterations) so a region blocked "exhausted —
 * unrouted" resumes iterating. The daemon resumes the run as part of the command.
 */
export async function bumpRegion(
  runId: string,
  regionId: string,
  additionalIter: number,
): Promise<void> {
  const resp = await fetch(`${BASE}/runs/${encodeURIComponent(runId)}/commands`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      kind: "bump_region",
      region_id: regionId,
      additional_iter: additionalIter,
    }),
  });
  if (!resp.ok) throw new Error(`bump_region failed: ${resp.status}`);
}

export async function retryAll(runId: string): Promise<CreateRunResponse> {
  const resp = await fetch(`${BASE}/runs/${encodeURIComponent(runId)}/commands`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ kind: "retry_all" }),
  });
  if (!resp.ok) throw new Error(`retry_all failed: ${resp.status}`);
  return resp.json();
}

export interface StartNodeResult {
  ok: boolean;
  iter?: number;
  already_running?: boolean;
}

export async function startNode(
  runId: string,
  nodeId: string,
): Promise<StartNodeResult> {
  const resp = await fetch(
    `${BASE}/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/start`,
    { method: "POST" },
  );
  if (!resp.ok) {
    const body = await resp.json().catch(() => null);
    throw new Error(body?.error ?? `start_node failed: ${resp.status}`);
  }
  return resp.json();
}

export async function stopNode(
  runId: string,
  nodeId: string,
): Promise<void> {
  const resp = await fetch(
    `${BASE}/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/stop`,
    { method: "POST" },
  );
  if (!resp.ok) {
    const body = await resp.json().catch(() => null);
    throw new Error(body?.error ?? `stop_node failed: ${resp.status}`);
  }
}

export interface RetryNodeResult {
  ok: boolean;
  iter: number;
  invalidated: string[];
}

export async function retryNode(
  runId: string,
  nodeId: string,
): Promise<RetryNodeResult> {
  const resp = await fetch(
    `${BASE}/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/retry`,
    { method: "POST" },
  );
  if (!resp.ok) {
    const body = await resp.json().catch(() => null);
    throw new Error(body?.error ?? `retry_node failed: ${resp.status}`);
  }
  return resp.json();
}

export interface RetryPreviewResult {
  downstream: string[];
  affected_count: number;
  with_artifacts: string[];
}

export async function retryNodePreview(
  runId: string,
  nodeId: string,
): Promise<RetryPreviewResult> {
  const resp = await fetch(
    `${BASE}/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/retry/preview`,
  );
  if (!resp.ok) {
    const body = await resp.json().catch(() => null);
    throw new Error(body?.error ?? `retry_preview failed: ${resp.status}`);
  }
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

export async function renameRun(runId: string, name: string): Promise<void> {
  const resp = await fetch(`${BASE}/runs/${encodeURIComponent(runId)}/commands`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ kind: "rename_run", name }),
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
  port_type?: PortType;
  frontmatter?: Record<string, FrontmatterFieldDecl> | null;
  when?: Record<string, unknown> | null;
}

export function libraryPortToPortDef(port: LibraryPort, defaultSide: PortSide): PortDef {
  return {
    name: port.name,
    repeated: port.repeated,
    side: (port.side as PortSide) ?? defaultSide,
    ...(port.port_type ? { port_type: port.port_type } : {}),
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
  /// Parsed form of `yaml`, normalized by the daemon's pipeline parser.
  /// Divergence checks compare against this — never against the raw text,
  /// whose formatting (key order, parser-filled defaults, serializer drift)
  /// does not survive a round-trip.
  pipeline: PipelineDef;
  prompts: Record<string, string>;
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

// --- Diff API ---

export async function fetchRunDiff(runId: string): Promise<string> {
  const resp = await fetch(`${BASE}/runs/${encodeURIComponent(runId)}/diff`);
  if (!resp.ok) throw new Error(`GET /runs/${runId}/diff failed: ${resp.status}`);
  return resp.text();
}

export async function fetchNodeDiff(runId: string, nodeId: string): Promise<string> {
  const resp = await fetch(
    `${BASE}/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/diff`,
  );
  if (!resp.ok) throw new Error(`GET /runs/${runId}/nodes/${nodeId}/diff failed: ${resp.status}`);
  return resp.text();
}

export async function deleteLibraryPipeline(id: string): Promise<void> {
  const resp = await fetch(`${BASE}/library/pipelines/${encodeURIComponent(id)}`, {
    method: "DELETE",
  });
  if (!resp.ok) throw new Error(`DELETE /library/pipelines/${id} failed: ${resp.status}`);
}

export interface PromoteResult {
  id: string;
  drifted: boolean;
}

export async function promotePipeline(pipelineId: string): Promise<PromoteResult> {
  const resp = await fetch(
    `${BASE}/pipelines/${encodeURIComponent(pipelineId)}/promote`,
    { method: "POST" },
  );
  if (!resp.ok) {
    const body = await resp.json().catch(() => null);
    throw new Error(body?.error ?? `POST /pipelines/${pipelineId}/promote failed: ${resp.status}`);
  }
  return resp.json();
}
