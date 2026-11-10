import { useEffect, useRef, useState } from "react";
import { create } from "zustand";
import type { RunTranscriptHit } from "../../shared/generated/RunTranscriptHit";
import type { RunTranscriptSearch } from "../../shared/generated/RunTranscriptSearch";

/** Where a chosen hit leads: a node iteration's transcript, at one entry.
 *  `nonce` tells two choices of the same hit apart. */
export interface TranscriptFocus {
  runId: string;
  nodeId: string;
  iter: number;
  index: number;
  nonce: number;
}

export type SearchStatus = "idle" | "searching" | "done" | "error";

interface RunSearchState {
  /** The Run the search panel is open on, `null` when closed. */
  runId: string | null;
  query: string;
  status: SearchStatus;
  result: RunTranscriptSearch | null;
  error: string | null;
  /** The hit the arrow keys are on. */
  selected: number;
  /** The last hit chosen; the transcript it opens follows it. */
  focus: TranscriptFocus | null;
  /** The `nonce` of the last focus the node panel switched to. */
  tabApplied: number;
  open: (runId: string) => void;
  close: () => void;
  toggle: (runId: string) => void;
  setQuery: (query: string) => void;
  settle: (patch: Pick<RunSearchState, "status" | "result" | "error">) => void;
  select: (index: number) => void;
  choose: (runId: string, hit: RunTranscriptHit) => void;
  applyTab: (nonce: number) => void;
}

/**
 * The search over a Run's transcripts, as the Run view holds it: the panel's
 * query and answer, the counts the canvas shows on the nodes, the hit chosen.
 * Transient view state, never in the edit store's history.
 */
export const useRunSearchStore = create<RunSearchState>((set, get) => ({
  runId: null,
  query: "",
  status: "idle",
  result: null,
  error: null,
  selected: 0,
  focus: null,
  tabApplied: 0,
  open: (runId) => {
    if (get().runId === runId) return;
    set({ runId, query: "", status: "idle", result: null, error: null, selected: 0, focus: null });
  },
  // Closed, the search lets go of its hit: a transcript opened later is not
  // pulled back to it.
  close: () =>
    set({ runId: null, query: "", status: "idle", result: null, error: null, selected: 0, focus: null }),
  toggle: (runId) => (get().runId === runId ? get().close() : get().open(runId)),
  setQuery: (query) => set({ query, selected: 0 }),
  settle: (patch) => set({ ...patch, selected: 0 }),
  select: (selected) => set({ selected }),
  choose: (runId, hit) =>
    set((s) => ({
      focus: {
        runId,
        nodeId: hit.node_id,
        iter: hit.iter,
        index: hit.hit.index,
        nonce: (s.focus?.nonce ?? 0) + 1,
      },
    })),
  applyTab: (tabApplied) => set({ tabApplied }),
}));

/** The query the open search highlights in the transcripts of `runId` — the
 *  one its answer is for, not the one still being typed — or "". */
export function useSearchHighlight(runId: string): string {
  return useRunSearchStore((s) => (s.runId === runId && s.result ? s.result.query : ""));
}

/** How many hits `nodeId` has while the search is open, 0 otherwise. */
export function useNodeSearchMatches(nodeId: string): number {
  return useRunSearchStore((s) =>
    s.runId ? (s.result?.nodes.find((n) => n.node_id === nodeId)?.matched ?? 0) : 0,
  );
}

/** The hit chosen in the search, when it points into this node iteration's
 *  transcript. */
export function useTranscriptFocus(
  runId: string,
  nodeId: string,
  iter: number,
): TranscriptFocus | null {
  const focus = useRunSearchStore((s) => s.focus);
  return focus && focus.runId === runId && focus.nodeId === nodeId && focus.iter === iter
    ? focus
    : null;
}

/**
 * Calls `apply(iter)` once per hit chosen in the search that points into
 * `nodeId`'s transcripts: the node's panel switches to the Transcript tab, at
 * the hit's iteration. A panel mounted later for the same node does not
 * switch again.
 */
export function useFocusTab(runId: string, nodeId: string, apply: (iter: number) => void): void {
  const focus = useRunSearchStore((s) => s.focus);
  const applied = useRunSearchStore((s) => s.tabApplied);
  const markApplied = useRunSearchStore((s) => s.applyTab);
  useEffect(() => {
    if (!focus || focus.nonce === applied) return;
    if (focus.runId !== runId || focus.nodeId !== nodeId) return;
    markApplied(focus.nonce);
    apply(focus.iter);
  }, [focus, applied, runId, nodeId, apply, markApplied]);
}

/** Calls `onChosen` each time a hit is chosen in the search (not when it
 *  mounts on an earlier choice). */
export function useOnHitChosen(onChosen: () => void): void {
  const nonce = useRunSearchStore((s) => s.focus?.nonce ?? 0);
  const seen = useRef(nonce);
  useEffect(() => {
    if (nonce === seen.current) return;
    seen.current = nonce;
    onChosen();
  }, [nonce, onChosen]);
}

/** Calls `unfold` once per new `focusNonce`, in the render it arrives: the
 *  entry the search's hit lands on unfolds, then stays as the reader leaves it. */
export function useFocusUnfold(focusNonce: number | undefined, unfold: () => void): void {
  const [seen, setSeen] = useState<number | undefined>(undefined);
  if (focusNonce !== undefined && focusNonce !== seen) {
    setSeen(focusNonce);
    unfold();
  }
}
