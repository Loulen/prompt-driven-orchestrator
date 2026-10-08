/**
 * A node of a Run, as the node panel reads and drives it: its prompt, its
 * inputs and outputs, the canvas buttons (start, stop, retry and what a retry
 * would invalidate), and the files a user imports into it.
 */

import type { ImportFilesResponse } from "../../shared/generated/ImportFilesResponse";
import type { PortType } from "../../shared/generated/PortType";
import { request } from "../../shared/http/client";

export function fetchPrompt(
  runId: string,
  nodeId: string,
  iter: number,
): Promise<string> {
  return request<string>(
    "GET",
    `/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/prompt`,
    { query: { iter }, responseMode: "text", label: "GET prompt" },
  );
}

export interface FileInfo {
  path: string;
  exists: boolean;
  size: number | null;
  frontmatter: Record<string, unknown> | null;
  /** #796: on disk before this execution started (survived a same-iter
   *  re-spawn, or inherited through git from a previous run) — not a result of
   *  the session shown. Absent when false. */
  inherited?: boolean;
}

export interface PortIO {
  port: string;
  repeated: boolean;
  port_type?: PortType;
  /** CORE-45: an Output intermédiaire — written while the node works, never
   *  checked. Always false on an input. */
  intermediate?: boolean;
  files: FileInfo[];
}

export interface NodeIO {
  inputs: PortIO[];
  outputs: PortIO[];
}

export function fetchNodeIO(
  runId: string,
  nodeId: string,
  iter: number,
): Promise<NodeIO> {
  return request<NodeIO>(
    "GET",
    `/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/io`,
    { query: { iter }, label: "GET io" },
  );
}

export interface StartNodeResult {
  ok: boolean;
  iter?: number;
  already_running?: boolean;
}

export function startNode(
  runId: string,
  nodeId: string,
): Promise<StartNodeResult> {
  return request<StartNodeResult>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/start`,
    { label: "start_node" },
  );
}

export function stopNode(
  runId: string,
  nodeId: string,
): Promise<void> {
  return request<void>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/stop`,
    { responseMode: "void", label: "stop_node" },
  );
}

export interface RetryNodeResult {
  ok: boolean;
  iter: number;
  invalidated: string[];
}

export function retryNode(
  runId: string,
  nodeId: string,
): Promise<RetryNodeResult> {
  return request<RetryNodeResult>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/retry`,
    { label: "retry_node" },
  );
}

export interface RetryPreviewResult {
  downstream: string[];
  affected_count: number;
  with_artifacts: string[];
}

export function retryNodePreview(
  runId: string,
  nodeId: string,
): Promise<RetryPreviewResult> {
  return request<RetryPreviewResult>(
    "GET",
    `/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/retry/preview`,
    { label: "retry_preview" },
  );
}

/** Import files from this browser's machine into a node holding a live
 *  session. Refused by name (no live session, over `max_attachments_mb`), in
 *  which case nothing was written. */
export function importNodeFiles(
  runId: string,
  nodeId: string,
  iter: number,
  files: File[],
): Promise<ImportFilesResponse> {
  const form = new FormData();
  for (const file of files) form.append("files", file, file.name);
  return request<ImportFilesResponse>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/attachments`,
    { body: form, query: { iter }, label: "Import" },
  );
}
