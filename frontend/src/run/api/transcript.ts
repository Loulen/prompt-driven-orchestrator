/**
 * A node iteration's Transcript (CONTEXT.md): its pages of normalized entries,
 * and one tool call's input and output, read on demand.
 */

import type { ToolCallDetail } from "../../shared/generated/ToolCallDetail";
import type { TranscriptPage } from "../../shared/generated/TranscriptPage";
import { request } from "../../shared/http/client";

function base(runId: string, nodeId: string): string {
  return `/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/transcript`;
}

export function fetchTranscriptPage(
  runId: string,
  nodeId: string,
  iter: number,
  after: number,
  limit: number,
): Promise<TranscriptPage> {
  return request<TranscriptPage>("GET", base(runId, nodeId), {
    query: { iter, after, limit },
    label: "GET transcript",
  });
}

export function fetchToolCallDetail(
  runId: string,
  nodeId: string,
  iter: number,
  index: number,
): Promise<ToolCallDetail> {
  return request<ToolCallDetail>("GET", `${base(runId, nodeId)}/tools/${index}`, {
    query: { iter },
    label: "GET transcript tool",
  });
}
