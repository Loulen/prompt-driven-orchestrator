/**
 * What a Run holds: its artifacts, its refs, and one file at one of them.
 */

import type { RunRefs } from "../../shared/generated/RunRefs";
import { BASE, request } from "../../shared/http/client";

export function fetchArtifact(
  runId: string,
  relativePath: string,
): Promise<string> {
  return request<string>(
    "GET",
    `/runs/${encodeURIComponent(runId)}/artifact`,
    { query: { path: relativePath }, responseMode: "text", label: "GET artifact" },
  );
}

export function artifactUrl(runId: string, relativePath: string): string {
  return `${BASE}/runs/${encodeURIComponent(runId)}/artifact?path=${encodeURIComponent(relativePath)}`;
}

/** The artifact as an attachment: any format, saved under a readable name. */
export function artifactDownloadUrl(runId: string, relativePath: string): string {
  return `${artifactUrl(runId, relativePath)}&download=true`;
}

/** Every file of a port folder in one zip, named after its node and port. */
export function portZipUrl(runId: string, folder: string): string {
  return `${BASE}/runs/${encodeURIComponent(runId)}/artifact/zip?path=${encodeURIComponent(folder)}`;
}

/** The folder all `paths` share, or null when they are spread over several. */
export function commonFolder(paths: string[]): string | null {
  const folders = new Set(paths.map((p) => p.slice(0, Math.max(0, p.lastIndexOf("/")))));
  if (folders.size !== 1) return null;
  const [folder] = folders;
  return folder || null;
}

/** Full content of one file at a Run ref (#748); defaults to the Run tip. */
export function fetchRunFileAtRef(
  runId: string,
  path: string,
  ref?: string,
): Promise<string> {
  const params = new URLSearchParams({ path });
  if (ref) params.set("ref", ref);
  return request<string>(
    "GET",
    `/runs/${encodeURIComponent(runId)}/file?${params.toString()}`,
    { responseMode: "text", label: `GET /runs/${runId}/file` },
  );
}

/**
 * The Run's refs (#749, ADR-0067 §1): fork point, Run tip, every node delivery's
 * `before`/`after`, the live branch of a running isolated node — with the
 * ready-made delivery pairs. Ids are stable; SHAs are informative only.
 */
export function fetchRunRefs(runId: string): Promise<RunRefs> {
  return request<RunRefs>("GET", `/runs/${encodeURIComponent(runId)}/refs`, {
    label: `GET /runs/${runId}/refs`,
  });
}
