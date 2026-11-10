import { useCallback, useMemo } from "react";
import type { Node } from "@xyflow/react";
import type { RunState } from "../../shared/generated/RunState";
import { nodePlayKind } from "../../run/model/nodePlay";
import { useNodePlay, type PlayCandidate } from "../../run/store/useNodePlay";
import type { EditorPipelineDef } from "../model/pipeline";

/**
 * ADR-0086 — the ▶ of the cards of a Run: an on-demand node never launched
 * starts, a skipped node retries. Asked to the Autorisation d'opération once
 * per set of candidates; `withPlay` hangs the allowed ▶ on a derived card.
 */
export function useCanvasNodePlay(
  pipeline: EditorPipelineDef | undefined,
  runState: RunState | null,
) {
  const candidates = useMemo<PlayCandidate[]>(() => {
    if (!pipeline || !runState) return [];
    return pipeline.nodes.flatMap((n) => {
      const kind = nodePlayKind({
        onDemand: n.on_demand ?? false,
        status: runState.nodes[n.id]?.status ?? "pending",
        runStatus: runState.status,
      });
      return kind ? [{ nodeId: n.id, kind }] : [];
    });
  }, [pipeline, runState]);
  const nodePlay = useNodePlay(runState?.run_id ?? null, candidates);
  const { whyNotPlay, play } = nodePlay;

  const withPlay = useCallback(
    (card: Node): Node => {
      const candidate = candidates.find((c) => c.nodeId === card.id);
      if (!candidate) return card;
      // CORE-205: a refused ▶ stays, greyed, with the refusal's sentence.
      return {
        ...card,
        data: {
          ...card.data,
          play: candidate.kind,
          playRefused: whyNotPlay(candidate.nodeId, candidate.kind),
          onPlay: () => void play(candidate.nodeId, candidate.kind),
        },
      };
    },
    [candidates, whyNotPlay, play],
  );

  return { withPlay, nodePlay };
}
