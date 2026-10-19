/**
 * The instance pipeline registry over HTTP: list, read, save, rename, create,
 * duplicate and delete a pipeline.
 */

import type { PipelineListEntry } from "../../shared/generated/PipelineListEntry";
import type { PipelineValidation } from "../../shared/generated/PipelineValidation";
import { ApiError, request } from "../../shared/http/client";
import type { PipelineDetail } from "../model/pipeline";
import { foldPipelineDetail } from "../model/pipelineDetail";

export function fetchPipelines(): Promise<PipelineListEntry[]> {
  return request<PipelineListEntry[]>("GET", "/pipelines");
}

// Pin an operation to a single store. Without it the daemon resolves a bare id
// repo-then-user, so a `library` (or `user`) entry colliding with a same-named
// repo pipeline routes to the wrong file (#216). `repo`/`user`/`run` map to the
// historical default and are only forwarded when explicitly known.
export function fetchPipeline(id: string, scope?: string): Promise<PipelineDetail> {
  void scope;
  return request<PipelineDetail>(
    "GET",
    `/pipelines/${encodeURIComponent(id)}`,
    { label: `GET /pipelines/${id}` },
  ).then(foldPipelineDetail);
}

export function savePipeline(
  id: string,
  yaml: string,
  prompts: Record<string, string>,
  scope?: string,
  draft?: { pipeline: unknown; prompts: Record<string, string> },
): Promise<{ ok: boolean; id?: string; renamed?: boolean }> {
  void scope;
  // #774 — the daemon answers with the FINAL id: a save whose `name:` field
  // changed moves the registry entry (`<old>.yaml` + sidecar) and returns the
  // new stem, so the caller can rekey the open tab in the same gesture.
  return request<{ ok: boolean; id?: string; renamed?: boolean }>(
    "PUT",
    `/pipelines/${encodeURIComponent(id)}`,
    // ADR-0088: a tab's shared draft is saved with what it held, which the
    // daemon then counts the draft's changes from.
    { body: draft ? { yaml, prompts, draft } : { yaml, prompts }, label: `PUT /pipelines/${id}` },
  );
}

/**
 * #774 — rename a pipeline: the visible name and the backing `.yaml` file move
 * together (visible name 1:1 with the file stem, so two distinct pipelines can
 * never share a name). 409 when the target stem or visible name is already
 * taken, or when active runs reference the pipeline.
 */
export function renamePipeline(
  id: string,
  name: string,
): Promise<{ ok: boolean; id: string; name?: string; renamed?: boolean }> {
  return request<{ ok: boolean; id: string; name?: string; renamed?: boolean }>(
    "PUT",
    `/pipelines/${encodeURIComponent(id)}/rename`,
    { body: { name }, label: `PUT /pipelines/${id}/rename` },
  );
}

export function createPipeline(
  name: string,
  scope?: string,
): Promise<{ id: string; scope: string; path: string }> {
  void scope;
  return request<{ id: string; scope: string; path: string }>(
    "POST",
    "/pipelines",
    { body: { name }, label: "POST /pipelines" },
  );
}

/**
 * Replace a Pipeline's validation (`PUT /pipelines/{id}/validation`, CONTEXT.md
 * « Pipeline validé »). An empty Projet list comes back as `{ kind: "none" }`;
 * an unknown Projet is a 400 naming it.
 */
export function setPipelineValidation(
  id: string,
  validation: PipelineValidation,
): Promise<{ ok: boolean; id: string; validation: PipelineValidation }> {
  return request<{ ok: boolean; id: string; validation: PipelineValidation }>(
    "PUT",
    `/pipelines/${encodeURIComponent(id)}/validation`,
    { body: validation, label: `PUT /pipelines/${id}/validation` },
  );
}

export function duplicatePipeline(
  id: string,
): Promise<{ id: string; scope: string; path: string }> {
  return request("POST", `/pipelines/${encodeURIComponent(id)}/duplicate`, {
    label: `POST /pipelines/${id}/duplicate`,
  });
}

export async function deletePipeline(id: string, scope?: string): Promise<void> {
  void scope;
  // Status-inspecting: a 409 (active runs) carries the reason in the body, so
  // raw mode keeps the bespoke branch (incl. the deliberately UNGUARDED 409
  // json). The old `{ conflict }` field had no reader — folded into status 409.
  const resp = await request<Response>(
    "DELETE",
    `/pipelines/${encodeURIComponent(id)}`,
    { responseMode: "raw" },
  );
  if (resp.status === 409) {
    const body = await resp.json();
    throw new ApiError(body.error ?? "Pipeline has active runs", { status: 409, body });
  }
  if (!resp.ok) throw new ApiError(`DELETE /pipelines/${id} failed: ${resp.status}`, { status: resp.status });
}
