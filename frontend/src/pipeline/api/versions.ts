/**
 * The versions of a registry pipeline over HTTP (ADR-0088): the history a
 * Save freezes, and one version, read only.
 */

import type { Actor } from "../../shared/generated/Actor";
import type { PipelineVersionEntry } from "../../shared/generated/PipelineVersionEntry";
import { request } from "../../shared/http/client";
import type { EditorPipelineDef } from "../model/pipeline";
import { foldPipelineDetail } from "../model/pipelineDetail";

/** How a Save lands among the versions. */
export interface SaveAs {
  /** `false`: the last version is replaced, under its number. */
  new_version: boolean;
  /** Blank or absent is the default title (`vN`). */
  title?: string;
  /** The version saved until now is kept as a pipeline of its own. */
  keep_old: boolean;
}

/** One version, as the editor shows it read only. */
export interface PipelineVersionDetail {
  number: number;
  title: string;
  author: Actor | null;
  saved_at: string;
  external: boolean;
  yaml: string;
  pipeline: EditorPipelineDef;
  prompts: Record<string, string>;
}

/** The history of `id`, newest first. */
export async function fetchPipelineVersions(id: string): Promise<PipelineVersionEntry[]> {
  const body = await request<{ versions: PipelineVersionEntry[] }>(
    "GET",
    `/pipelines/${encodeURIComponent(id)}/versions`,
  );
  return body.versions;
}

export async function fetchPipelineVersion(id: string, number: number): Promise<PipelineVersionDetail> {
  const detail = await request<PipelineVersionDetail>(
    "GET",
    `/pipelines/${encodeURIComponent(id)}/versions/${number}`,
  );
  // Folded like every pipeline the editor loads (#550).
  const folded = foldPipelineDetail({
    id,
    scope: "instance",
    path: "",
    yaml: detail.yaml,
    pipeline: detail.pipeline,
    prompts: detail.prompts,
    diagnostics: [],
  });
  return { ...detail, pipeline: folded.pipeline };
}
