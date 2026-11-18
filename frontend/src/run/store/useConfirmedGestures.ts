import { useCallback, useState } from "react";
import type { NodeStatus } from "../../shared/generated/NodeStatus";
import {
  nodeGestureConfirmation,
  type ConfirmedGesture,
  type GestureConfirmation,
} from "../model/nodeGestureConfirmation";

/** A gesture asked and not answered yet: what the window says, and what Confirm sends. */
export interface PendingGesture {
  confirmation: GestureConfirmation;
  send: () => void;
}

/**
 * The node panel's destructive gestures behind their confirmation (CORE-206).
 * `ask(gesture, send)` opens the window `nodeGestureConfirmation` rules for
 * it, or sends at once when it rules none. `pending` is the window to show;
 * `confirm` sends its gesture, `cancel` drops it and sends nothing.
 */
export function useConfirmedGestures(status: NodeStatus, nodeName: string) {
  const [pending, setPending] = useState<PendingGesture | null>(null);
  const ask = useCallback(
    (gesture: ConfirmedGesture, send: () => void) => {
      const confirmation = nodeGestureConfirmation({ gesture, status, nodeName });
      if (confirmation) setPending({ confirmation, send });
      else send();
    },
    [status, nodeName],
  );
  const confirm = useCallback(() => {
    setPending(null);
    pending?.send();
  }, [pending]);
  const cancel = useCallback(() => setPending(null), []);
  return { ask, pending, confirm, cancel };
}
