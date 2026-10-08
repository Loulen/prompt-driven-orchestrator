import { useEffect, useRef, useState } from "react";
import type { MutableRefObject } from "react";
import type { RunState } from "../../shared/generated/RunState";
import { fetchRun } from "../../run/api/runs";
import { useDaemonSocket } from "../../run/store/useDaemonSocket";
import { deliverySignature } from "../../run/model/runRefs";
import type { RefPair } from "../../run/model/runRefs";

/** How long a card keeps its outline flash after a live reply. */
const FLASH_MS = 2400;

interface Args {
  runId: string;
  runRef: MutableRefObject<RunState | null>;
  setRun: (r: RunState) => void;
  sigAtLoad: MutableRefObject<string | null>;
  pairRef: MutableRefObject<RefPair>;
  onTipMoved: () => void;
}

/**
 * The Run's WebSocket, as the Review page listens to it: every event of the
 * Run refreshes it (and so does a `resync`), to detect "tip moved" (the delivery signature changed since
 * the diff was loaded) and "node delivered" (the live branch on display is
 * gone), and to flash the card of a reply that landed live (#751).
 */
export function useReviewLive({ runId, runRef, setRun, sigAtLoad, pairRef, onTipMoved }: Args) {
  const [nodeDelivered, setNodeDelivered] = useState<{ nodeId: string; iter: number } | null>(null);
  const [flashing, setFlashing] = useState<ReadonlySet<string>>(() => new Set());
  const flashTimers = useRef<Map<string, number>>(new Map());
  const onTipMovedRef = useRef(onTipMoved);
  useEffect(() => {
    onTipMovedRef.current = onTipMoved;
  }, [onTipMoved]);
  useEffect(() => {
    const timers = flashTimers.current;
    return () => timers.forEach((t) => window.clearTimeout(t));
  }, []);

  const { subscribe } = useDaemonSocket();
  useEffect(() => {
    return subscribe((msg) => {
      // ADR-0083 §6: a `resync` (the stream reopened, the tab shows again, the
      // network is back, or the daemon lost messages) reads the Run again as
      // one of its events would.
      const resync = msg.type === "resync";
      if (!resync && (msg.type !== "event" || !msg.event || msg.event.run_id !== runId)) return;
      const ev = resync ? null : msg.event;
      fetchRun(runId)
        .then((r) => {
          const before = runRef.current;
          setRun(r);
          const sig = deliverySignature(r.nodes);
          if (sigAtLoad.current !== null && sig !== sigAtLoad.current) onTipMovedRef.current();
          // #751: a reply landed live — flash its card for a moment. The dot
          // follows from `seen` (the snapshot does not know this reply).
          if (before) {
            const prevReplies = new Map((before.review_comments ?? []).map((c) => [c.id, c.replies?.length ?? 0]));
            const grown = (r.review_comments ?? []).filter((c) => (c.replies?.length ?? 0) > (prevReplies.get(c.id) ?? 0));
            if (grown.length > 0) {
              setFlashing((f) => new Set([...f, ...grown.map((c) => c.id)]));
              for (const c of grown) {
                window.clearTimeout(flashTimers.current.get(c.id));
                flashTimers.current.set(
                  c.id,
                  window.setTimeout(() => {
                    setFlashing((f) => {
                      const next = new Set(f);
                      next.delete(c.id);
                      return next;
                    });
                  }, FLASH_MS),
                );
              }
            }
          }
        })
        .catch(() => {});
      if (ev?.kind === "node_delivered" && ev.node_id && pairRef.current.to === `live:${ev.node_id}`) {
        setNodeDelivered({ nodeId: ev.node_id, iter: ev.iter ?? 1 });
      }
    });
  }, [subscribe, runId, runRef, setRun, sigAtLoad, pairRef]);

  return { nodeDelivered, setNodeDelivered, flashing };
}
