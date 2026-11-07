import { useEffect } from "react";
import { create } from "zustand";
import type { NodeState } from "../../shared/generated/NodeState";

/** A deep link's request to show a node at an iteration; `nonce` tells two links apart. */
interface NodeLinkRequest {
  runId: string;
  nodeId: string;
  /** The iteration the link names; `null`: the node's current one. */
  iter: number | null;
  nonce: number;
}

interface NodeLinkFocusState {
  request: NodeLinkRequest | null;
  /** The `nonce` of the last request a node panel followed. */
  applied: number;
  open: (runId: string, nodeId: string, iter: number | null) => void;
  apply: (nonce: number) => void;
}

/**
 * A deep link toward a node (`#/runs/<run>/nodes/<node>/iter/<n>`) selects the
 * node, and the node's panel (mounted already, or mounted by that selection)
 * shows the iteration the link names. Transient view state.
 */
export const useNodeLinkFocus = create<NodeLinkFocusState>((set) => ({
  request: null,
  applied: 0,
  open: (runId, nodeId, iter) =>
    set((s) => ({ request: { runId, nodeId, iter, nonce: (s.request?.nonce ?? 0) + 1 } })),
  apply: (applied) => set({ applied }),
}));

/**
 * Calls `select(iter)` once per link aimed at `node` of `runId`: the iteration
 * it names when the node ran it (one it never ran leaves the panel as it is),
 * its current one when it names none.
 */
export function useNodeLinkIter(
  runId: string,
  node: Pick<NodeState, "node_id" | "iter" | "iterations">,
  select: (iter: number) => void,
): void {
  const request = useNodeLinkFocus((s) => s.request);
  const applied = useNodeLinkFocus((s) => s.applied);
  const markApplied = useNodeLinkFocus((s) => s.apply);
  const { node_id: nodeId, iter: current, iterations } = node;
  useEffect(() => {
    if (!request || request.nonce === applied) return;
    if (request.runId !== runId || request.nodeId !== nodeId) return;
    markApplied(request.nonce);
    // No iteration named: the current one, where the node waits.
    const iter = request.iter ?? current;
    if (iter === current || (iterations ?? []).some((it) => it.iter === iter)) select(iter);
  }, [request, applied, runId, nodeId, current, iterations, select, markApplied]);
}
