import type { SelectionKind } from "../stores/editStore";
import type { RunStatus } from "../types";
import { isNodeActiveRun } from "../types";

/**
 * Decide whether the App should auto-snap the canvas selection to the run's
 * latest live node (App's "see the terminal at once" effect).
 *
 * The snap fires ONLY when the pane is not already owned by a deliberate choice
 * and the run has an actively-running node:
 *
 *  - A node is already selected (`"node"` with an id) — leave it be.
 *  - An explicit inspector is open — `"region"` / `"edge"` / `"note"` (#150 /
 *    #147 / #307) or `"run"` (the Run-info / Repositories sidebar, #465 slice 2,
 *    F1). Each is the user opening something on purpose; the snap must not steal
 *    the pane back. `"run"` is the load-bearing one for F1: without it, the
 *    Repositories editor would be unreachable while a node runs, because the
 *    snap would re-select the node the instant the sidebar opened.
 *  - The run is `running` / `awaiting_user` (`isNodeActiveRun`) — a `paused` or
 *    terminal run does not auto-snap, so its sidebar is already reachable by
 *    deselecting.
 *
 * Returning `true` means "attempt the snap"; the caller still resolves the
 * concrete node (`pickLatestLiveNode`) and no-ops if none exists.
 */
export function shouldAutoSnapToLiveNode(
  selectionKind: SelectionKind,
  hasNodeId: boolean,
  runStatus: RunStatus,
): boolean {
  if (selectionKind === "node" && hasNodeId) return false;
  if (
    selectionKind === "region" ||
    selectionKind === "edge" ||
    selectionKind === "note" ||
    selectionKind === "run"
  ) {
    return false;
  }
  return isNodeActiveRun(runStatus);
}
