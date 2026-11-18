/**
 * A node gesture that destroys something is confirmed first (CORE-206,
 * « Geste de nœud confirmé » in CONTEXT.md). This rule says, for a gesture on
 * a node, whether it asks and with what words: one rule for every surface
 * that offers the gesture (the node panel's controls, its banners, the
 * Ctrl+K palette), one window to show it (`GestureConfirmDialog`).
 *
 * Stop, Restart clean and the stale node's Stop always ask: each ends an
 * agent session. A Retry asks only when it ends a live session (`running`,
 * `awaiting_user`) or resets downstream nodes that hold artifacts; the
 * recovery Retry of a settled node with nothing downstream goes at once.
 */

import type { NodeStatus } from "../../shared/generated/NodeStatus";

/** The node gestures this rule rules on. */
export type ConfirmedGesture = "stop" | "restart" | "kill_stale" | "retry";

/** The (i) after the consequence: its name, and the sentence its tooltip says. */
export interface GestureInfo {
  label: string;
  description: string;
}

/** What the confirmation window says, and how. */
export interface GestureConfirmation {
  /** The action and its object: « Stop node implement? ». */
  title: string;
  /** One line per consequence, a few words each. */
  consequences: string[];
  /** The failed tone on the action: every gesture that asks destroys something. */
  danger: boolean;
  /** The action button. */
  actionLabel: string;
  /** The (i) at the end of the last consequence, when the gesture has more to say. */
  info?: GestureInfo;
}

/** What a Stop leaves of the agent's conversation (D2, D9 of CORE-206). */
export const STOP_INFO: GestureInfo = {
  label: "Ends this conversation",
  description:
    "Retry starts a fresh agent. Depending on the harness, you can still resume this conversation by hand " +
    "in the run's worktree (e.g. claude --resume) until the run is archived.",
};

export interface GestureOnNode {
  gesture: ConfirmedGesture;
  /** The node's projected status. */
  status: NodeStatus;
  /** The node's name, else its id. */
  nodeName: string;
  /**
   * A Retry's downstream nodes that hold artifacts, which it resets (the
   * daemon's retry preview, `affected_count`). Read by the Retry alone.
   */
  downstreamWithArtifacts?: number;
}

/** The statuses whose agent session is live: a Retry ends it. */
const LIVE_STATUSES: ReadonlySet<NodeStatus> = new Set(["running", "awaiting_user"]);

/**
 * The confirmation `gesture` asks on this node, or `null` when it goes at
 * once. Stop, Restart clean and the stale Stop always ask; a Retry asks
 * when it ends a live session or resets downstream nodes, with one line per
 * consequence.
 */
export function nodeGestureConfirmation({
  gesture,
  status,
  nodeName,
  downstreamWithArtifacts = 0,
}: GestureOnNode): GestureConfirmation | null {
  switch (gesture) {
    case "retry": {
      const consequences: string[] = [];
      if (LIVE_STATUSES.has(status)) consequences.push("Ends its live session");
      if (downstreamWithArtifacts > 0) {
        const n = downstreamWithArtifacts;
        consequences.push(`Resets ${n} downstream ${n === 1 ? "node" : "nodes"}`);
      }
      if (consequences.length === 0) return null;
      return { title: `Retry node ${nodeName}?`, consequences, danger: true, actionLabel: "Retry" };
    }
    case "stop":
      return {
        title: `Stop node ${nodeName}?`,
        consequences: ["Ends its agent session"],
        danger: true,
        actionLabel: "Stop",
        info: STOP_INFO,
      };
    case "restart":
      return {
        title: `Restart node ${nodeName}?`,
        consequences: ["Ends its session, restarts clean"],
        danger: true,
        actionLabel: "Restart",
      };
    case "kill_stale":
      return {
        title: `Stop stale node ${nodeName}?`,
        consequences: ["Marks it failed"],
        danger: true,
        actionLabel: "Stop",
      };
  }
}
