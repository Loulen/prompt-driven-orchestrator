/**
 * A Run's own pipeline: read, edited mid-run, exported as a portable document
 * with its skills sidecar.
 */

import type { PipelineDetail } from "../../pipeline/model/pipeline";
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
