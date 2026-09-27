/**
 * The retired pipeline library (`/library/pipelines`, promote): kept as the
 * front still calls it; the daemon answers these routes 404 since the
 * registry unification (#774).
 */

import { request } from "../../shared/http/client";
import type { EditorPipelineDef } from "../model/pipeline";

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
  pipeline: EditorPipelineDef;
  prompts: Record<string, string>;
}

export function fetchLibraryPipelines(): Promise<LibraryPipelineEntry[]> {
  return request<LibraryPipelineEntry[]>("GET", "/library/pipelines");
}

export interface SaveLibraryPipelineOptions {
  /// When set, save in-place at this id even if `name` changed. Required for
  /// rename-in-place: without it the daemon falls back to slug(name), which
  /// would orphan the previous entry.
  id?: string;
  scope?: LibraryPipelineScope;
}

export function saveLibraryPipeline(
  name: string,
  yaml: string,
  prompts: Record<string, string> = {},
  options: SaveLibraryPipelineOptions = {},
): Promise<{ id: string; scope: LibraryPipelineScope }> {
  return request<{ id: string; scope: LibraryPipelineScope }>(
    "POST",
    "/library/pipelines",
    {
      body: {
        name,
        yaml,
        prompts,
        ...(options.id ? { id: options.id } : {}),
        ...(options.scope ? { scope: options.scope } : {}),
      },
      label: "POST /library/pipelines",
    },
  );
}

/// Duplicate a library pipeline template into an unlinked clone: fresh id, name
/// suffixed `(copy)` / `(copy N)`, no promotion metadata (#224). Returns the new
/// id, its scope, and the freshly-listed library entry (or null if the list
/// race-loses the just-created file).
export function duplicateLibraryPipeline(
  id: string,
): Promise<{ id: string; scope: LibraryPipelineScope; entry: LibraryPipelineEntry | null }> {
  return request<{ id: string; scope: LibraryPipelineScope; entry: LibraryPipelineEntry | null }>(
    "POST",
    `/library/pipelines/${encodeURIComponent(id)}/duplicate`,
  );
}

export function deleteLibraryPipeline(id: string): Promise<void> {
  return request<void>(
    "DELETE",
    `/library/pipelines/${encodeURIComponent(id)}`,
    { responseMode: "void", label: `DELETE /library/pipelines/${id}` },
  );
}

export interface PromoteResult {
  id: string;
  drifted: boolean;
}

export function promotePipeline(pipelineId: string): Promise<PromoteResult> {
  return request<PromoteResult>(
    "POST",
    `/pipelines/${encodeURIComponent(pipelineId)}/promote`,
    { label: `POST /pipelines/${pipelineId}/promote` },
  );
}
