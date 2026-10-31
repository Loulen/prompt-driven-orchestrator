import { useEffect } from "react";
import { create } from "zustand";

/** A request to show a node's Orchestration tab; `nonce` tells two clicks apart. */
interface OrchestrationRequest {
  runId: string;
  nodeId: string;
  nonce: number;
}

interface OrchestrationFocusState {
  request: OrchestrationRequest | null;
  /** The `nonce` of the last request a node panel switched to. */
  applied: number;
  open: (runId: string, nodeId: string) => void;
  apply: (nonce: number) => void;
}

/**
 * The « N runs » row of an orchestrator's card opens its Orchestration tab. The
 * card selects the node, and the node's panel (mounted already, or mounted by
 * that selection) switches to the tab. Transient view state.
 */
export const useOrchestrationFocus = create<OrchestrationFocusState>((set) => ({
  request: null,
  applied: 0,
  open: (runId, nodeId) =>
    set((s) => ({ request: { runId, nodeId, nonce: (s.request?.nonce ?? 0) + 1 } })),
  apply: (applied) => set({ applied }),
}));

/** Calls `apply()` once per request aimed at `nodeId` of `runId`. */
export function useOrchestrationFocusTab(runId: string, nodeId: string, apply: () => void): void {
  const request = useOrchestrationFocus((s) => s.request);
  const applied = useOrchestrationFocus((s) => s.applied);
  const markApplied = useOrchestrationFocus((s) => s.apply);
  useEffect(() => {
    if (!request || request.nonce === applied) return;
    if (request.runId !== runId || request.nodeId !== nodeId) return;
    markApplied(request.nonce);
    apply();
  }, [request, applied, runId, nodeId, apply, markApplied]);
}
