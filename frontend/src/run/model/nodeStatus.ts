import type { NodeState } from "../../shared/generated/NodeState";
import type { NodeStatus } from "../../shared/generated/NodeStatus";
import type { RunStatus } from "../../shared/generated/RunStatus";
import type { MarkVerdict } from "../store/useNodeRun";

/**
 * The statuses Approve is offered on, in the Inspector as in the
 * command field: those the daemon completes (its transition guard takes a
 * `running`, `awaiting_user` or `failed` iteration). An `interrupted` node
 * re-drives on Reopen, a `stale` one (historical Runs) on Retry: the daemon
 * refuses to complete either.
 */
const MARK_COMPLETABLE: ReadonlySet<NodeStatus> = new Set(["awaiting_user", "running", "failed"]);

/**
 * Whether Approve is offered on a node of this status, in a Run of
 * this status: the daemon reopens a settled Run to complete it, never a
 * paused one (resumed first) nor an archived one.
 */
export const isMarkCompletable = (status: NodeStatus, run: RunStatus): boolean =>
  MARK_COMPLETABLE.has(status) && run !== "paused" && run !== "archived";

/**
 * Three tones, because the three answers demand three different reactions:
 * `await` = it is still your turn, `failed` = the node is failed *now*,
 * `stopped` = refused with no state change, or indeterminable.
 */
export function verdictTone(v: MarkVerdict): "await" | "failed" | "stopped" | "done" {
  switch (v.kind) {
    case "completed":
      return "done";
    case "noop":
      return "stopped";
    case "pending":
      return "stopped";
    case "error":
      return "failed";
    case "refused":
      if (v.recoverable === true) return "await";
      if (v.recoverable === false) return "failed";
      return "stopped";
  }
}

export function verdictHeadline(v: MarkVerdict): string {
  switch (v.kind) {
    case "pending":
      return "Approving…";
    case "completed":
      return "Approved.";
    case "noop":
      return `Already complete — nothing to do (${v.reason})`;
    case "error":
      return `Could not reach the daemon — ${v.message}`;
    case "refused":
      if (v.recoverable === true) return `Refused, still your turn — ${v.message}`;
      if (v.recoverable === false) return `Refused, the node is now failed — ${v.message}`;
      return `Refused — ${v.message}`;
  }
}

/**
 * Does this node's failure come from output validation? (#490)
 *
 * `includes`, **not** an equality or a `startsWith`: the two reasons are
 * `"output validation failed"` (after retry) and `"script output validation failed"`
 * (the `script` fail-fast), and the second neither equals nor starts with the first.
 * Both banner gates use this one predicate, inverted — two separate tests would let
 * both fire, or neither.
 */
export function isOutputValidationFailure(node: NodeState): boolean {
  return node.failure_reason?.includes("output validation failed") ?? false;
}

// The session is settled — the tmux session is gone, so the terminal WebSocket
// would attach to a dead session. `{completed, skipped, failed, stopped,
// interrupted}` is exactly the "settled" tier of `pollInterval` (5s). A `skipped`
// node never held a session at all (#620); `interrupted` belongs here too (#598 /
// ADR-0049): its tmux session is *definitively* dead (that death is what produced
// the status), so attaching would only spam "can't find session" — open minimized
// and prioritise the Outputs, exactly like the other settled states. A Retry
// revives the session and flips back to split (`onRetryStarted`). `stale` is
// EXCLUDED unless archived: its tmux session is typically still alive and recovery
// happens *inside* the terminal (nudge / Stop / Retry). An archived run overrides
// everything (its worktree + session are torn down). An unknown future status
// falls on the non-terminated (live) side.
export function nodeSessionEnded(status: NodeStatus, isArchived?: boolean): boolean {
  if (isArchived) return true;
  return (
    status === "completed" ||
    status === "skipped" ||
    status === "failed" ||
    status === "stopped" ||
    status === "interrupted"
  );
}

export function terminalPlaceholder(node: NodeState): string {
  switch (node.status) {
    case "pending":
      return "Not started yet";
    case "waiting":
      // Admission-throttled: ready, but the session cap is reached.
      return "Waiting for a free session slot";
    case "completed":
      return "Session ended.";
    case "skipped":
      // #620: never ran — its producing branch was not taken.
      return `Skipped: ${node.skip_reason ?? "branch not taken"}`;
    case "failed":
      return `Failed: ${node.failure_reason ?? "unknown reason"}`;
    case "stopped":
      return `Stopped: ${node.failure_reason ?? "user stopped"}`;
    case "stale":
      return "Agent idle — outputs incomplete";
    case "interrupted":
      // #598 / ADR-0049: the session died on an infra incident, the work is
      // presumed intact — Reopen/Retry re-drives it.
      return `Interrupted: ${node.failure_reason ?? "session died — reopen or retry"}`;
    case "running":
      return "Connecting...";
    case "awaiting_user":
      return "Waiting for user interaction...";
  }
}
