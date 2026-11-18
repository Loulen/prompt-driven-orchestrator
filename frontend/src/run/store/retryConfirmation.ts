import type { NodeStatus } from "../../shared/generated/NodeStatus";
import { retryNodePreview } from "../api/nodes";
import { nodeGestureConfirmation, type GestureConfirmation } from "../model/nodeGestureConfirmation";

/** The node a Retry is asked on. */
export interface RetryTarget {
  runId: string;
  nodeId: string;
  /** The node's projected status. */
  status: NodeStatus;
  /** The node's name, else its id. */
  nodeName: string;
}

/**
 * The confirmation a Retry of this node asks (CORE-206), or `null` when it
 * goes at once: the daemon's retry preview says how many downstream nodes
 * with artifacts it resets, `nodeGestureConfirmation` rules on it with the
 * node's status. The one Retry decision of every surface (node panel and
 * banners, canvas ▶, Ctrl+K palette). A preview that fails rejects.
 */
export async function retryConfirmation({ runId, nodeId, status, nodeName }: RetryTarget): Promise<GestureConfirmation | null> {
  const preview = await retryNodePreview(runId, nodeId);
  return nodeGestureConfirmation({
    gesture: "retry",
    status,
    nodeName,
    downstreamWithArtifacts: preview.affected_count,
  });
}
