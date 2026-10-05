// What a terminal shows of its session: the live PTY, or the pane the daemon
// froze when it reaped the session (#205, #617). Pure: no xterm, no socket.

import type { Dimensions } from "./altBufferResize";

/** Which node iteration's frozen pane to read when the live session is gone (#617). */
export interface PaneSource {
  runId: string;
  nodeId: string;
  iter: number;
}

// What this terminal is showing. `probing` is the beat before the daemon has said
// whether the reaped iteration left a snapshot behind; `live` is the PTY attach.
export type PaneMode = "probing" | "live" | "frozen";

/** The frozen pane, when `mode` is `frozen`. */
export interface FrozenPane {
  content: string;
  /** `false` ⇒ the daemon has no snapshot either; say so instead of pretending. */
  preserved: boolean;
}

// A node iteration in one of these states has had its tmux session reaped on the
// terminal transition (#205, the one-live-iteration invariant), so attaching a PTY
// to it can only produce tmux's `can't find session:` on a dead socket. What
// survives is the snapshot the daemon froze on the way out — and until #617 the UI
// never asked for it, so the primary surface of every finished node was that error
// string. The set mirrors the daemon's own terminal iteration in the pane read:
// `interrupted` is absent from both, its session may still be alive.
export const REAPED_STATUSES = new Set(["completed", "failed", "stopped", "stale"]);

// #876: `body` sets `letter-spacing: -0.005em` and `font-feature-settings`, and
// both inherit into xterm. The DOM renderer measures "W" with a DOM span (which
// inherits the spacing) while the cell width comes from canvas `measureText`
// (which ignores it), then sets `.xterm-rows { letter-spacing: cell − span }`.
// The inherited -0.065px turns into +0.058px per glyph, so a full row overflows
// its `overflow: hidden` div and the last 1–2 columns are clipped at the right
// edge. Resetting both on the container, which exists before `term.open()`,
// makes the two measures agree. Neither the grid size nor FitAddon is at fault.
/** Left breathing room for the grid, in px (#911). */
export const TERMINAL_INSET_PX = 4;

export const TERMINAL_TYPOGRAPHY_RESET = {
  letterSpacing: "normal",
  fontFeatureSettings: "normal",
} as const;

// xterm writes raw bytes: a snapshot captured with `tmux capture-pane -pe` is
// newline-separated, and a bare \n moves down without returning, so every line
// would start where the previous one ended. Normalise to CRLF without doubling
// the \r of a line that already carries one.
export function toTerminalNewlines(content: string): string {
  return content.replace(/\r?\n/g, "\r\n");
}

// Send a resize message to the daemon, but only if the dimensions are valid.
// FitAddon.proposeDimensions() can momentarily return 0-rows/0-cols during
// a transient layout pass (container attached but not yet measured). The
// daemon's resize decoder rejects zero values, and historically would treat
// the rejected JSON as user input — injecting stray characters into whatever
// has focus in tmux. Guarding here closes that hole at the source.
export function sendResize(ws: WebSocket, dims: Dimensions | undefined): void {
  if (ws.readyState !== WebSocket.OPEN) return;
  if (!dims) return;
  if (!Number.isFinite(dims.cols) || !Number.isFinite(dims.rows)) return;
  if (dims.cols <= 0 || dims.rows <= 0) return;
  ws.send(
    JSON.stringify({ type: "resize", cols: dims.cols, rows: dims.rows }),
  );
}

/** The toolbar's status dot and label. */
export function paneIndicator(
  mode: PaneMode,
  connected: boolean,
  status: string | undefined,
  frozen: FrozenPane | null,
): { dotClass: string; statusLabel: string } {
  const isActive =
    status === "running" || status === "awaiting_user" || status === "stale";
  if (mode === "probing") {
    return { dotClass: "bg-fg-5", statusLabel: "reading pane…" };
  }
  if (mode === "frozen") {
    // Named, not dressed up as a connection: what is on screen is the pane PDO
    // froze when it reaped the session, and the reap is the invariant, not a fault.
    return {
      dotClass: "bg-fg-5",
      statusLabel: frozen?.preserved ? "snapshot · session reaped" : "no pane kept",
    };
  }
  if (!connected) return { dotClass: "bg-fg-5", statusLabel: "disconnected" };
  if (isActive) {
    return { dotClass: "animate-pulse bg-st-running", statusLabel: "attached · live" };
  }
  return { dotClass: "bg-st-done", statusLabel: "connected" };
}
