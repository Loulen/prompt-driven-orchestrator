/**
 * The life-cycle groupings of a Run status, shared by every view of a Run.
 */

import type { RunStatus } from "../../shared/generated/RunStatus";

export function isLiveRun(status: RunStatus): boolean {
  return status === "running" || status === "awaiting_user" || status === "paused";
}

/**
 * A run with an actively-running (or user-awaiting) node — the states in which
 * the App auto-snaps the selection to that node so its terminal is shown at
 * once. NARROWER than `isLiveRun`: a `paused` run does not auto-snap, so its
 * Run-info panel is already reachable by deselecting. This is exactly the set
 * where the panel is otherwise unreachable, so the canvas exposes an explicit
 * toggle for it (#465 slice 2, F1). Keep the auto-snap guard and that toggle's
 * visibility on this single predicate so they can never drift apart.
 */
export function isNodeActiveRun(status: RunStatus): boolean {
  return status === "running" || status === "awaiting_user";
}

/**
 * Mirror of Rust `RunStatus::is_terminal()` (the total complement of `is_live`):
 * `{completed, failed, skipped, halted, archived}`. NOTE this INCLUDES
 * `archived` — callers that gate on "terminal AND not archived" (e.g. the
 * "Open session" shell action, #316) must exclude `archived` explicitly.
 */
export function isTerminalRun(status: RunStatus): boolean {
  return !isLiveRun(status);
}
