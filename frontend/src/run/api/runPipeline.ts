/**
 * A Run's own pipeline: read, edited mid-run, copied over the shared pipeline
 * it came from (ADR-0085), exported as a portable document with its skills
 * sidecar.
 */

import type { PipelineDetail } from "../../pipeline/model/pipeline";
import type { OverwritePreview } from "../../shared/generated/OverwritePreview";
import { foldPipelineDetail } from "../../pipeline/model/pipelineDetail";
import { sidecarBlob } from "../../shared/http/sidecar";
import { request } from "../../shared/http/client";

export function fetchRunPipeline(runId: string): Promise<PipelineDetail> {
  return request<PipelineDetail>(
    "GET",
    `/runs/${encodeURIComponent(runId)}/pipeline`,
    { label: `GET /runs/${runId}/pipeline` },
  ).then(foldPipelineDetail);
}

export function saveRunPipeline(
  runId: string,
  yaml: string,
  prompts: Record<string, string>,
): Promise<void> {
  return request<void>(
    "PUT",
    `/runs/${encodeURIComponent(runId)}/pipeline`,
    { body: { yaml, prompts }, responseMode: "void", label: `PUT /runs/${runId}/pipeline` },
  );
}

/** ADR-0085: what « Overwrite default pipeline » would touch, read before the warning. */
export function fetchRunPipelineOverwritePreview(runId: string): Promise<OverwritePreview> {
  return request<OverwritePreview>(
    "GET",
    `/runs/${encodeURIComponent(runId)}/pipeline/overwrite-preview`,
    { label: `GET /runs/${runId}/pipeline/overwrite-preview` },
  );
}

/** ADR-0085: the Run's whole snapshot (YAML and prompts) replaces the shared pipeline. */
export function overwriteDefaultPipelineFromRun(runId: string): Promise<{ pipeline_id: string }> {
  return request<{ pipeline_id: string }>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/pipeline/overwrite-default`,
    { label: `POST /runs/${runId}/pipeline/overwrite-default` },
  );
}

export function fetchRunPipelineDocument(runId: string): Promise<string> {
  return request("GET", `/runs/${encodeURIComponent(runId)}/pipeline/document`, {
    responseMode: "text",
    label: `GET /runs/${runId}/pipeline/document`,
  });
}

export async function fetchRunPipelineSkillsSidecar(runId: string): Promise<Blob | null> {
  const resp = await request<Response>(
    "GET",
    `/runs/${encodeURIComponent(runId)}/pipeline/document/skills`,
    { responseMode: "raw", label: `GET /runs/${runId}/pipeline/document/skills` },
  );
  return sidecarBlob(resp, `GET /runs/${runId}/pipeline/document/skills`);
}
