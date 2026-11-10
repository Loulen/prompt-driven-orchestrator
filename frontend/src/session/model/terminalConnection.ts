// ADR-0083 §5 — the connection of a live terminal: its delay of silence, the
// daemon's beat on its socket, and the veil over a terminal whose connection
// fell.

import type { PaneMode } from "./terminalPane";

/** The veil over a live terminal whose socket fell. */
export const CONNECTION_CLOSED_LABEL = "Terminal connection closed";
export const RECONNECT_LABEL = "Reconnect";

/**
 * The daemon beats every terminal socket every 10 s. Three missed beats and
 * the socket is dead, even though the browser never saw it close: a half-open
 * connection (laptop asleep, Wi-Fi gone behind a proxy) fires no `close`, and
 * would leave a frozen pane badged « attached · live ».
 */
export const TERMINAL_SILENCE_MS = 30_000;

/**
 * How long a fallen connection waits before the veil comes up. A node that
 * finishes ends its session, and the daemon closes the socket before the
 * node's new status reaches the browser: without this grace, the veil would
 * flash over every node that settles under the user's eyes.
 */
export const VEIL_GRACE_MS = 2_000;

/** Whether a Text frame is the daemon's `{"type":"heartbeat"}` beat: it only
 *  feeds the delay of silence and is never written to the pane. */
export function isHeartbeatFrame(text: string): boolean {
  try {
    const msg: unknown = JSON.parse(text);
    return typeof msg === "object" && msg !== null && (msg as { type?: unknown }).type === "heartbeat";
  } catch {
    return false;
  }
}

/** Whether a live terminal is veiled: its socket was open and fell (closed,
 *  offline or silent), and the node did not settle — a settled node's session
 *  ends on purpose, which is no fault to veil. */
export function isVeiled(mode: PaneMode, dropped: boolean, settled: boolean): boolean {
  return mode === "live" && dropped && !settled;
}

/** The statuses of a node whose session is up. */
const LIVE_STATUSES = new Set(["running", "awaiting_user"]);

/**
 * Whether the node's session came back: its status turned live from one that
 * is not (a restart or a recovery of an `interrupted` node, a retry of a
 * `failed` one, under the same session name). The terminal whose socket fell
 * with the old session reconnects then, by itself — it stayed on « Terminal
 * connection closed » until Reconnect (FP of CORE-227). `running` ⇄
 * `awaiting_user` is the same session.
 */
export function sessionCameBack(previous: string | undefined, next: string | undefined): boolean {
  return next !== undefined && LIVE_STATUSES.has(next) && previous !== undefined && !LIVE_STATUSES.has(previous);
}
