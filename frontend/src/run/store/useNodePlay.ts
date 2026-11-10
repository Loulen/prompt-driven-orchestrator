import { useCallback, useEffect, useMemo, useState } from "react";
import { askAuthorization } from "../../shared/authorization/api";
import { toast } from "../../shared/ui/toast";
import { retryNode, retryNodePreview, startNode } from "../api/nodes";
import { playKey, playQuestion, refusedPlays, type PlayKind } from "../model/nodePlay";

export interface PlayCandidate {
  nodeId: string;
  kind: PlayKind;
}

/**
 * The ▶ of the nodes of one Run (ADR-0086): which ones the Autorisation
 * d'opération refuses, and why — asked once per set of candidates, nothing
 * greyed until the answer arrives, as without extension (CORE-205) — and the
 * gesture itself.
 *
 * A retry that would reset downstream nodes with artifacts asks first, like
 * the panel's Retry; a refusal of the daemon is said in a toast.
 */
export function useNodePlay(runId: string | null, candidates: PlayCandidate[]) {
  const key = useMemo(
    () => (runId ? `${runId}|${candidates.map((c) => playKey(c.nodeId, c.kind)).join(",")}` : ""),
    [runId, candidates],
  );
  const [rights, setRights] = useState<{ key: string; refused: Map<string, string> } | null>(null);
  const [retryConfirm, setRetryConfirm] = useState<{ nodeId: string; affectedCount: number } | null>(
    null,
  );

  useEffect(() => {
    if (!runId || candidates.length === 0) return;
    let cancelled = false;
    askAuthorization(candidates.map((c) => playQuestion(runId, c.nodeId, c.kind)))
      .then((decisions) => {
        if (!cancelled) setRights({ key, refused: refusedPlays(candidates, decisions) });
      })
      .catch(() => {
        // Unanswered: nothing greyed, the daemon still decides.
      });
    return () => {
      cancelled = true;
    };
  }, [runId, candidates, key]);

  /** Why the caller may not play this ▶ (the refusal's sentence), `undefined` when allowed. */
  const whyNotPlay = useCallback(
    (nodeId: string, kind: PlayKind) =>
      rights?.key === key ? rights.refused.get(playKey(nodeId, kind)) : undefined,
    [rights, key],
  );
  const canPlay = useCallback(
    (nodeId: string, kind: PlayKind) => whyNotPlay(nodeId, kind) === undefined,
    [whyNotPlay],
  );

  const refused = useCallback((kind: PlayKind, e: unknown) => {
    const message = e instanceof Error ? e.message : String(e);
    toast(`${kind === "start" ? "Start" : "Retry"} refused — ${message}`, "error");
  }, []);

  const play = useCallback(
    async (nodeId: string, kind: PlayKind) => {
      if (!runId) return;
      try {
        if (kind === "start") {
          await startNode(runId, nodeId);
          return;
        }
        const preview = await retryNodePreview(runId, nodeId);
        if (preview.affected_count > 0) {
          setRetryConfirm({ nodeId, affectedCount: preview.affected_count });
          return;
        }
        await retryNode(runId, nodeId);
      } catch (e) {
        refused(kind, e);
      }
    },
    [runId, refused],
  );

  const confirmRetry = useCallback(async () => {
    const pending = retryConfirm;
    setRetryConfirm(null);
    if (!runId || !pending) return;
    try {
      await retryNode(runId, pending.nodeId);
    } catch (e) {
      refused("retry", e);
    }
  }, [runId, retryConfirm, refused]);

  const cancelRetry = useCallback(() => setRetryConfirm(null), []);

  return { canPlay, whyNotPlay, play, retryConfirm, confirmRetry, cancelRetry };
}
