/**
 * The Transcript tab's view of a node iteration: the entries loaded so far
 * and the counts of the whole transcript, merged page by page.
 */

import type { TranscriptEntry } from "../../shared/generated/TranscriptEntry";
import type { TranscriptPage } from "../../shared/generated/TranscriptPage";

export interface TranscriptView {
  available: boolean;
  harness: string | null;
  entries: TranscriptEntry[];
  total: number;
  messages: number;
  toolCalls: number;
}

export const EMPTY_TRANSCRIPT: TranscriptView = {
  available: true,
  harness: null,
  entries: [],
  total: 0,
  messages: 0,
  toolCalls: 0,
};

/** Fold a page read from entry `after` on into the view: its entries replace
 *  the loaded ones from `after`, the counts follow the daemon's. */
export function mergePage(
  view: TranscriptView,
  after: number,
  page: TranscriptPage,
): TranscriptView {
  const head = view.entries.slice(0, after);
  const tail = view.entries.slice(after + page.entries.length);
  return {
    available: page.available,
    harness: page.harness ?? view.harness,
    entries: [...head, ...page.entries, ...tail],
    total: page.total,
    messages: page.messages,
    toolCalls: page.tool_calls,
  };
}

/** Fold only the counts of a page: what a live read past the loaded entries
 *  learns while the reader has not scrolled that far. */
export function mergeCounts(view: TranscriptView, page: TranscriptPage): TranscriptView {
  return {
    ...view,
    available: page.available,
    harness: page.harness ?? view.harness,
    total: page.total,
    messages: page.messages,
    toolCalls: page.tool_calls,
  };
}

/** Where a live refresh reads from: the first tool call still running (its
 *  result may have landed), else the end of what is loaded. A running call
 *  further back than one `window` (a session that died in it) is left
 *  behind, so the read always reaches the end. */
export function refreshFrom(view: TranscriptView, window = Infinity): number {
  const end = view.entries.length;
  const pending = view.entries.find(
    (e) => e.kind === "tool" && e.status === "pending" && end - e.index <= window,
  );
  return pending ? pending.index : end;
}

/** The entries not loaded yet. */
export function remaining(view: TranscriptView): number {
  return Math.max(0, view.total - view.entries.length);
}

/** `2.1 kB`, `512 B`: a tool output's size. */
export function formatBytes(bytes: number): string {
  if (bytes < 1000) return `${bytes} B`;
  if (bytes < 1_000_000) return `${(bytes / 1000).toFixed(1)} kB`;
  return `${(bytes / 1_000_000).toFixed(1)} MB`;
}
