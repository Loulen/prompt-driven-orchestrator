/**
 * The ▶ a node of a Run offers (ADR-0086): an on-demand node not launched yet
 * is started, a skipped node is retried — « Forcer un nœud skipped », the
 * branch then runs as if it had been taken. Both re-open a finished Run.
 *
 * The ▶ shows on a live or finished Run, never on an archived one; greyed,
 * with the refusal's sentence, when the Autorisation d'opération would refuse
 * it (`node` × `start` for an on-demand node, `node` × `retry` for a skipped
 * one; CORE-205).
 */

import type { NodeStatus } from "../../shared/generated/NodeStatus";
import type { RunStatus } from "../../shared/generated/RunStatus";
import type { Decision, OperationQuestion } from "../../shared/authorization/api";
import { refusalOf } from "../../shared/authorization/api";

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

/** The key of a ▶ in the refusals. */
export function playKey(nodeId: string, kind: PlayKind): string {
  return `${nodeId}:${kind}`;
}

/** The ▶ the `decisions` to `questions` refuse, with each refusal's sentence, keyed by {@link playKey}. */
export function refusedPlays(
  asked: { nodeId: string; kind: PlayKind }[],
  decisions: Decision[],
): Map<string, string> {
  const refused = new Map<string, string>();
  asked.forEach((play, i) => {
    const why = refusalOf(decisions[i]);
    if (why !== undefined) refused.set(playKey(play.nodeId, play.kind), why);
  });
  return refused;
}

/** The text of the wait of a Run past `End` (« Attente des nœuds lancés »). */
export function waitingForStartedNodesLabel(count: number): string {
  return `waiting for ${count} started ${count === 1 ? "node" : "nodes"}`;
}
