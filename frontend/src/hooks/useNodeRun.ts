import { useCallback, useEffect, useState } from "react";
import {
  markNodeDone,
  killNode,
  restartNode,
  startNode,
  stopNode,
  retryNode,
  retryNodePreview,
  fetchPrompt,
  fetchNodeIO,
} from "../api";
import type { PortIO, MarkNodeDoneOutcome } from "../api";
import type { NodeState, NodeStatus } from "../types";

function pollInterval(status: NodeStatus): number | null {
  switch (status) {
    case "running":
    case "awaiting_user":
    case "stale":
      return 1000;
    // #598 / ADR-0049: an interrupted node is settled but recoverable — poll it
    // at the slow terminal cadence (like the other terminal states) so the UI
    // updates when a human reopens/retries it.
    case "completed":
    case "skipped":
    case "failed":
    case "stopped":
    case "interrupted":
      return 5000;
    case "pending":
      return null;
  }
}

/**
 * What the last *Mark complete* click produced (#490, ADR-0035).
 *
 * `pending` exists so the verdict region always has a tenant: the handler no longer
 * clears before awaiting, so there is no window in which a previous verdict has been
 * erased and no new one written. `error` is the transport breakdown, which the
 * pre-#490 code swallowed into `console.error` and rendered as nothing.
 */
export type MarkVerdict =
  | { kind: "pending" }
  | MarkNodeDoneOutcome
  | { kind: "error"; message: string };

/**
 * What the last Retry/Play or Start click produced when it was REFUSED (#487
 * §"Sans le volet frontend, le correctif est invisible").
 *
 * Both handlers used to swallow their error in an empty `catch {}`: the daemon's
 * `409` — "resume the run first" for a terminal Run, "session cap reached" for a
 * force-spawn — arrived and rendered as nothing, so the operator clicked and saw no
 * change at all. This is the verdict the panel now renders at the gesture, exactly
 * as `markVerdict` does for Mark complete (#490). Success needs no tenant here (the
 * node flips to `running` and the terminal re-shows), so `null` is the resting
 * state; only a refusal writes one.
 */
export type ActionVerdict = {
  action: "retry" | "start" | "restart";
  message: string;
};

export interface UseNodeRunOptions {
  isArchived?: boolean;
  /**
   * Fired once a retry has actually been sent (#346 seam). The terminal inset's
   * display mode stays presentational state in `NodeDetailPanel`, but a retry
   * revives the session and must re-show the terminal beside the details — so
   * the flip travels back out as a callback instead of dragging `terminalView`
   * into this hook. NOT called on the confirm-dialog path, where nothing has
   * been retried yet.
   */
  onRetryStarted?: () => void;
}

/**
 * Run-command orchestration for one node of one run: the per-iter prompt and IO
 * reads, plus every command the node detail panel can send (stop / retry /
 * start / mark complete, and the two stale-banner actions).
 *
 * `selectedIter` is an **argument, not state**. It is both an orchestration
 * input (every read and `mark_node_done` is iter-scoped) and the IterSelector's
 * UI state; the panel owns the selector (`userSelectedIter`, which falls back to
 * `node.iter` when the selection belongs to another node) and passes the
 * resolved iter down, so there is exactly one owner and no second copy to keep
 * in sync.
 *
 * The archive guard gates the **prompt fetch only** (#315): the IO effect never
 * looks at `isArchived`, so an archived node still reports its preserved
 * artifacts — one-shot for a pending one, on the `pollInterval` cadence
 * otherwise.
 */
export function useNodeRun(
  runId: string,
  node: NodeState,
  selectedIter: number,
  { isArchived, onRetryStarted }: UseNodeRunOptions,
) {
  const [promptText, setPromptText] = useState<string | null>(null);
  const [inputs, setInputs] = useState<PortIO[]>([]);
  const [outputs, setOutputs] = useState<PortIO[]>([]);
  // #490: a VERDICT object, not `string[] | null`. The old shape was
  // *structurally incapable* of expressing "refused for a reason that has no port
  // list" — which is the bug: the transition guard's refusal ("resume the run
  // first") arrived with an empty list and the banner was gated on `length > 0`, so
  // the most frequent refusal of all rendered nothing.
  //
  // Scoped by `iter` on the `userSelectedIter` idiom of the detail panel, which also
  // kills a latent second bug: a verdict from iter 3 surviving a switch back to iter 1.
  const [markVerdict, setMarkVerdict] = useState<
    ({ iter: number } & MarkVerdict) | null
  >(null);
  const [retryConfirm, setRetryConfirm] = useState<{
    affectedCount: number;
  } | null>(null);
  // #487: the refusal of the last Retry/Play or Start click, rendered at the
  // gesture. `null` while nothing was refused.
  const [actionVerdict, setActionVerdict] = useState<ActionVerdict | null>(null);

  const interval = pollInterval(node.status);
  const isStaleIter = selectedIter !== node.iter;

  // #315: the per-iter *rendered* prompt lives in the node's working dir, which
  // is destroyed on archive and is not among the preserved set (ADR-0020 keeps
  // artifacts + pipeline.yaml + pipeline.prompts/). So the fetch would always
  // 404 for an archived run — skip it and show an honest note instead of a
  // stuck "Loading prompt..." spinner.
  const shouldFetchPrompt =
    !isArchived && (node.status !== "pending" || isStaleIter);

  useEffect(() => {
    if (!shouldFetchPrompt) return;

    let cancelled = false;

    fetchPrompt(runId, node.node_id, selectedIter)
      .then((text) => {
        if (!cancelled) setPromptText(text);
      })
      .catch(() => {});

    return () => {
      cancelled = true;
    };
  }, [runId, node.node_id, selectedIter, shouldFetchPrompt]);

  useEffect(() => {
    const oneShot = isStaleIter || (interval === null && node.status === "pending");

    if (oneShot) {
      let cancelled = false;
      fetchNodeIO(runId, node.node_id, selectedIter)
        .then((io) => {
          if (!cancelled) {
            setInputs(io.inputs);
            setOutputs(io.outputs);
          }
        })
        .catch(() => {});
      return () => {
        cancelled = true;
      };
    }

    if (interval === null) return;

    let cancelled = false;

    async function pollIO() {
      try {
        const io = await fetchNodeIO(runId, node.node_id, selectedIter);
        if (!cancelled) {
          setInputs(io.inputs);
          setOutputs(io.outputs);
        }
      } catch {
        // ignore
      }
    }

    pollIO();
    const timer = setInterval(pollIO, interval);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [interval, node.node_id, selectedIter, runId, isStaleIter, node.status]);

  const stop = useCallback(async () => {
    try {
      await stopNode(runId, node.node_id);
    } catch {
      // best-effort
    }
  }, [runId, node.node_id]);

  const retry = useCallback(async () => {
    // #487: a fresh attempt clears the previous refusal, then either writes a new
    // one or resolves silently. The daemon's `409` ("resume the run first") is no
    // longer swallowed — it becomes the rendered verdict below.
    setActionVerdict(null);
    try {
      const preview = await retryNodePreview(runId, node.node_id);
      if (preview.affected_count > 0) {
        setRetryConfirm({ affectedCount: preview.affected_count });
        return; // not retried yet → leave terminalView untouched
      }
      await retryNode(runId, node.node_id);
      // Session revives (status → running once refreshRun lands the NodeStarted
      // event); re-show the terminal beside the details rather than full-frame.
      onRetryStarted?.();
    } catch (e) {
      setActionVerdict({
        action: "retry",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, [runId, node.node_id, onRetryStarted]);

  const confirmRetry = useCallback(async () => {
    setRetryConfirm(null);
    setActionVerdict(null);
    try {
      await retryNode(runId, node.node_id);
      onRetryStarted?.();
    } catch (e) {
      setActionVerdict({
        action: "retry",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, [runId, node.node_id, onRetryStarted]);

  const cancelRetry = useCallback(() => {
    setRetryConfirm(null);
  }, []);

  // #204: force-spawn a pending node out of dependency order. The daemon owns
  // the run-status gate (a non-spawnable run returns 409). #487: that 409 —
  // swallowed before, so a click on a pending node in a terminal run looked like
  // it did nothing — is now surfaced as a rendered verdict, like Retry.
  const start = useCallback(async () => {
    setActionVerdict(null);
    try {
      await startNode(runId, node.node_id);
    } catch (e) {
      setActionVerdict({
        action: "start",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, [runId, node.node_id]);

  const markComplete = useCallback(async () => {
    // #490: NO `setMarkVerdict(null)` preamble. Clearing before awaiting is what
    // made the banner flicker — on a lying `200` nothing was written back, so a
    // verdict from a previous click vanished and never returned. The region always
    // has a tenant: `pending` occupies it, and every exit writes a verdict.
    const iter = selectedIter;
    setMarkVerdict({ iter, kind: "pending" });
    try {
      const outcome: MarkNodeDoneOutcome = await markNodeDone(runId, node.node_id, iter);
      setMarkVerdict({ iter, ...outcome });
    } catch (e) {
      // No longer swallowed into `console.error`: a POST that never lands must not
      // look like a success.
      setMarkVerdict({
        iter,
        kind: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, [runId, node.node_id, selectedIter]);

  // The two stale-banner actions (ADR-0032 §1: historical runs only). Iter-scoped
  // like the reads — the banner acts on the iteration on screen, not on
  // `node.iter`.
  const killStale = useCallback(async () => {
    try { await killNode(runId, node.node_id, selectedIter); } catch { /* best-effort */ }
  }, [runId, node.node_id, selectedIter]);

  const restartIteration = useCallback(async () => {
    setActionVerdict(null);
    try {
      await restartNode(runId, node.node_id, selectedIter);
      onRetryStarted?.();
    } catch (e) {
      setActionVerdict({
        action: "restart",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, [runId, node.node_id, selectedIter, onRetryStarted]);

  return {
    promptText,
    inputs,
    outputs,
    markVerdict,
    actionVerdict,
    retryConfirm,
    stop,
    retry,
    confirmRetry,
    cancelRetry,
    start,
    markComplete,
    killStale,
    restartIteration,
  };
}
