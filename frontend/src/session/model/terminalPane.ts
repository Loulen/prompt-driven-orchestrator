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

// A row with nothing to read: blank, or only colour resets. A row painted with a
// background colour (a TUI's status bar) is not empty, and stays.
const COLOUR_RESETS = ["\u001b[0m", "\u001b[m", "\u001b[39m", "\u001b[49m"];
const isEmptyRow = (row: string) =>
  COLOUR_RESETS.reduce((rest, reset) => rest.replaceAll(reset, ""), row).trim() === "";

// What a frozen pane writes into xterm. xterm writes raw bytes: a snapshot captured
// with `tmux capture-pane -pe` is newline-separated, and a bare \n moves down without
// returning, so every line would start where the previous one ended: rows are joined
// with CRLF. And `capture-pane` returns the whole pane (CORE-232): a script's few
// lines come followed by the empty rows under them. Written as is, they overflowed a
// small terminal, and xterm, which keeps its last row in view, showed the empty rows
// with the lines in its scrollback. The empty rows after the last line go, and so
// does the final newline, which would scroll one row more.
export function frozenPaneText(content: string): string {
  const rows = content.split(/\r?\n/);
  while (rows.length > 0 && isEmptyRow(rows[rows.length - 1])) rows.pop();
  return rows.join("\r\n");
}

// Send a resize message to the daemon, but only if the dimensions are valid.
// FitAddon.proposeDimensions() can momentarily return 0-rows/0-cols during
// a transient layout pass (container attached but not yet measured). The
// daemon's resize decoder rejects zero values, and historically would treat
// the rejected JSON as user input — injecting stray characters into whatever
// has focus in tmux. Guarding here closes that hole at the source.
export function sendResize(ws: { send(data: string): boolean }, dims: Dimensions | undefined): void {
  if (!dims) return;
  if (!Number.isFinite(dims.cols) || !Number.isFinite(dims.rows)) return;
  if (dims.cols <= 0 || dims.rows <= 0) return;
  // Not open yet: nothing is sent, the opening sends the size.
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
    return { dotClass: "bg-text-3", statusLabel: "reading pane…" };
  }
  if (mode === "frozen") {
    // Named, not dressed up as a connection: what is on screen is the pane PDO
    // froze when it reaped the session, and the reap is the invariant, not a fault.
    return {
      dotClass: "bg-text-3",
      statusLabel: frozen?.preserved ? "snapshot · session reaped" : "no pane kept",
    };
  }
  if (!connected) return { dotClass: "bg-text-3", statusLabel: "disconnected" };
  if (isActive) {
    return { dotClass: "animate-pulse bg-st-running", statusLabel: "attached · live" };
  }
  return { dotClass: "bg-st-done", statusLabel: "connected" };
}
