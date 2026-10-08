/**
 * The ▶ a node of a Run offers (ADR-0086): an on-demand node not launched yet
 * is started, a skipped node is retried — « Forcer un nœud skipped », the
 * branch then runs as if it had been taken. Both re-open a finished Run.
 *
 * The ▶ shows on a live or finished Run, never on an archived one, and only
 * when the Autorisation d'opération would allow it (`node` × `start` for an
 * on-demand node, `node` × `retry` for a skipped one).
 */

import type { NodeStatus } from "../../shared/generated/NodeStatus";
import type { RunStatus } from "../../shared/generated/RunStatus";
import type { Decision, OperationQuestion } from "../../shared/authorization/api";
import { isAllowed } from "../../shared/authorization/api";

/** What the ▶ of a node does: start it (iteration 1) or retry it. */
export type PlayKind = "start" | "retry";

/**
 * What the ▶ of a node does, or `null` when it has none. `status` is the
 * node's projected status, `pending` for a node the Run never launched.
 */
export function nodePlayKind({
  onDemand,
  status,
  runStatus,
}: {
  onDemand: boolean;
  status: NodeStatus;
  runStatus: RunStatus | null | undefined;
}): PlayKind | null {
  if (runStatus == null || runStatus === "archived") return null;
  if (status === "skipped") return "retry";
  if (onDemand && status === "pending") return "start";
  return null;
}

/** The Operation behind the ▶ of `nodeId`, as `POST /authorize` reads it. */
export function playQuestion(runId: string, nodeId: string, kind: PlayKind): OperationQuestion {
  return { resource: "node", verb: { action: kind }, target: `${runId}/${nodeId}` };
}

/** The key of a ▶ in the set of allowed ones. */
export function playKey(nodeId: string, kind: PlayKind): string {
  return `${nodeId}:${kind}`;
}

/** The ▶ the `decisions` to `questions` allow, keyed by {@link playKey}. */
export function allowedPlays(
  asked: { nodeId: string; kind: PlayKind }[],
  decisions: Decision[],
): Set<string> {
  const allowed = new Set<string>();
  asked.forEach((play, i) => {
    if (isAllowed(decisions[i])) allowed.add(playKey(play.nodeId, play.kind));
  });
  return allowed;
}

/** The text of the wait of a Run past `End` (« Attente des nœuds lancés »). */
export function waitingForStartedNodesLabel(count: number): string {
  return `waiting for ${count} started ${count === 1 ? "node" : "nodes"}`;
}
