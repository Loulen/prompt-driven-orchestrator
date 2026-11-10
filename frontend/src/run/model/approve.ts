import type { NodeState } from "../../shared/generated/NodeState";
import type { NodeStatus } from "../../shared/generated/NodeStatus";
import type { RunStatus } from "../../shared/generated/RunStatus";
import { isMarkCompletable } from "./nodeStatus";

/**
 * Approve ▾, the one completion gesture of a node, said the same wherever it
 * is offered (the node panel, the enlarged terminal, the command field).
 */
export const APPROVE_LABEL = "Approve";
export const AUTO_APPROVE_LABEL = "Auto-approve when ready";
export const AUTO_APPROVE_ARMED = "Auto-approve armed";
export const AUTO_APPROVE_ARMING = "Arming auto-approve…";

/** Whether Approve ▾ shows at all: on the latest iteration, while the daemon would complete the node. */
export function approveShown(node: NodeState, selectedIter: number, runStatus: RunStatus): boolean {
  return selectedIter === node.iter && isMarkCompletable(node.status, runStatus);
}

/**
 * Whether « Auto-approve when ready » applies to an iteration: an interactive
 * one whose session lives.
 */
export function approveReleasable(
  iteration: { interactive?: boolean | null; status: NodeStatus } | undefined,
  fallbackStatus: NodeStatus,
): boolean {
  const status = iteration?.status ?? fallbackStatus;
  return iteration?.interactive === true && (status === "running" || status === "awaiting_user");
}

/** The outputs of an iteration that are not on the blackboard yet. */
export function missingOutputs(outputs: readonly { port: string; missing?: boolean }[]): string[] {
  return outputs.filter((p) => p.missing).map((p) => p.port);
}

/**
 * Why Approve cannot run yet, from the outputs of the iteration: `null` while
 * they are being read, then the missing ones; `undefined` once nothing is
 * missing.
 */
export function approveWaitReason(missing: readonly string[] | null): string | undefined {
  if (missing === null) return "Reading the outputs…";
  return missing.length > 0 ? `Missing outputs: ${missing.join(", ")}` : undefined;
}
