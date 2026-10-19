/**
 * The search over a Run's transcripts (CONTEXT.md « Transcript »): the seam
 * that opens it, and what the panel and the canvas make of an answer.
 */

import type { RunTranscriptHit } from "../../shared/generated/RunTranscriptHit";
import type { RunTranscriptSearch } from "../../shared/generated/RunTranscriptSearch";

export const RUN_SEARCH_EVENT = "pdo:run-search";

/** How long after the last keystroke the search is sent, in ms. */
export const SEARCH_DEBOUNCE_MS = 250;

/** The most hits the daemon answers. */
export const SEARCH_MAX_HITS = 200;

export interface RunSearchRequest {
  runId: string;
}

/** Ask the Run view to open the search over `runId`'s transcripts. */
export function openRunSearch(runId: string): void {
  window.dispatchEvent(
    new CustomEvent<RunSearchRequest>(RUN_SEARCH_EVENT, { detail: { runId } }),
  );
}

/** ⌘⇧F on a Mac, Ctrl+Shift+F elsewhere. */
export function isSearchShortcut(e: Pick<KeyboardEvent, "metaKey" | "ctrlKey" | "shiftKey" | "altKey" | "key">): boolean {
  return (e.metaKey || e.ctrlKey) && e.shiftKey && !e.altKey && e.key.toLowerCase() === "f";
}

/** The hits of one node iteration, in transcript order. */
export interface HitGroup {
  nodeId: string;
  iter: number;
  /** The position of the group's first hit in the whole list. */
  first: number;
  hits: RunTranscriptHit[];
}

/** The hits grouped by node iteration, in the daemon's order (by node, then
 *  chronology). */
export function groupHits(hits: RunTranscriptHit[]): HitGroup[] {
  const groups: HitGroup[] = [];
  hits.forEach((hit, i) => {
    const last = groups[groups.length - 1];
    if (last && last.nodeId === hit.node_id && last.iter === hit.iter) last.hits.push(hit);
    else groups.push({ nodeId: hit.node_id, iter: hit.iter, first: i, hits: [hit] });
  });
  return groups;
}

/** `9 in 3 nodes`, `1 in 1 node`. */
export function countLabel(result: RunTranscriptSearch): string {
  const nodes = result.nodes.length;
  return `${result.matched} in ${nodes} ${nodes === 1 ? "node" : "nodes"}`;
}

/** The hit chosen after an arrow key: `delta` steps away, kept in the list. */
export function moveSelection(current: number, delta: number, total: number): number {
  if (total === 0) return 0;
  return Math.min(total - 1, Math.max(0, current + delta));
}
