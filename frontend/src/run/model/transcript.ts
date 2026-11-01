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
 *  the loaded ones at the same positions, the others stay, in transcript
 *  order — a page read toward either end (the next one, an older one, a live
 *  re-read) lands where it belongs, never twice. The counts follow the
 *  daemon's. */
export function mergePage(
  view: TranscriptView,
  after: number,
  page: TranscriptPage,
): TranscriptView {
  const from = after;
  const to = after + page.entries.length;
  const kept = view.entries.filter((e) => e.index < from || e.index >= to);
  const entries = [...kept, ...page.entries].sort((a, b) => a.index - b.index);
  return {
    available: page.available,
    harness: page.harness ?? view.harness,
    entries,
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

/** The position just past the newest loaded entry (0 when none is). */
export function loadedEnd(view: TranscriptView): number {
  const last = view.entries[view.entries.length - 1];
  return last ? last.index + 1 : 0;
}

/** Where a live refresh reads from: the first tool call still running (its
 *  result may have landed), else the end of what is loaded. A running call
 *  further back than one `window` (a session that died in it) is left
 *  behind, so the read always reaches the end. */
export function refreshFrom(view: TranscriptView, window = Infinity): number {
  const end = loadedEnd(view);
  const pending = view.entries.find(
    (e) => e.kind === "tool" && e.status === "pending" && end - e.index <= window,
  );
  return pending ? pending.index : end;
}

/** The entries not loaded yet, on either side. */
export function remaining(view: TranscriptView): number {
  return Math.max(0, view.total - view.entries.length);
}

/** The entries newer than the loaded ones (all of them when none is loaded). */
export function newerRemaining(view: TranscriptView): number {
  return Math.max(0, view.total - loadedEnd(view));
}

/** The entries older than the loaded ones (all of them when none is loaded:
 *  a view read from its end starts empty). */
export function olderRemaining(view: TranscriptView): number {
  const first = view.entries[0];
  return first ? first.index : view.total;
}

/** The read that loads the page just older than what is loaded — the last
 *  `size` entries of a view that has none yet — or null at the start. */
export function olderPage(view: TranscriptView, size: number): { after: number; limit: number } | null {
  const end = olderRemaining(view);
  if (end <= 0) return null;
  const after = Math.max(0, end - size);
  return { after, limit: end - after };
}

/** `2.1 kB`, `512 B`: a tool output's size. */
export function formatBytes(bytes: number): string {
  if (bytes < 1000) return `${bytes} B`;
  if (bytes < 1_000_000) return `${(bytes / 1000).toFixed(1)} kB`;
  return `${(bytes / 1_000_000).toFixed(1)} MB`;
}
