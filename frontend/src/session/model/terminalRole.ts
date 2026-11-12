// #867 / ADR-0075 — the role frames of the terminal socket.
//
// The daemon pushes `{"type":"role",…}` on the socket's Text channel at every
// change: `solo` (one poste: nothing shown), `pilot` (with the number of other
// postes watching) or `spectator` (with the pilot's grid). A connection that may
// read the session but not attach to it is read-only whatever its role: its
// `solo` and `spectator` frames carry `read_only`, the sentence saying why.

import type { Dimensions } from "./altBufferResize";

/** #867 / ADR-0075: this terminal's role between postes, pushed by the daemon.
 *  `solo` is the default and shows nothing: one poste, or several tabs of it. */
export type TerminalRole =
  | { role: "solo"; readOnly?: string }
  | { role: "pilot"; spectators: number }
  | { role: "spectator"; pilot: Dimensions; readOnly?: string };

export const SOLO: TerminalRole = { role: "solo" };

/** Parse a `{"type":"role",…}` text frame; `null` for anything else. */
export function parseRoleFrame(text: string): TerminalRole | null {
  let msg: unknown;
  try {
    msg = JSON.parse(text);
  } catch {
    return null;
  }
  if (typeof msg !== "object" || msg === null) return null;
  const m = msg as Record<string, unknown>;
  if (m.type !== "role") return null;
  const readOnly = typeof m.read_only === "string" && m.read_only !== "" ? m.read_only : undefined;
  if (m.role === "solo") return readOnly ? { role: "solo", readOnly } : SOLO;
  if (m.role === "pilot") {
    const n = Number(m.spectators);
    return { role: "pilot", spectators: Number.isFinite(n) && n > 0 ? n : 0 };
  }
  if (m.role === "spectator") {
    const p = m.pilot as Record<string, unknown> | undefined;
    const cols = Number(p?.cols);
    const rows = Number(p?.rows);
    if (!(cols > 0) || !(rows > 0)) return null;
    return readOnly ? { role: "spectator", pilot: { cols, rows }, readOnly } : { role: "spectator", pilot: { cols, rows } };
  }
  return null;
}

export const READ_ONLY_HINT =
  "Read-only — another browser has control. Take control from the terminal bar.";

/** Why this connection may never type: the sentence of its refusal, if any. */
export function readOnlyOf(role: TerminalRole): string | undefined {
  return role.role === "pilot" ? undefined : role.readOnly;
}

/** Whether this connection's keystrokes are dropped: a spectator's, or a
 *  read-only connection's whatever its role. */
export function isWatching(role: TerminalRole): boolean {
  return role.role === "spectator" || readOnlyOf(role) !== undefined;
}

/** The screen's hint for a connection that may never type, `why` the sentence
 *  of its refusal. */
export function readOnlyHint(why: string): string {
  return `Read-only — you can't type in this terminal. ${why}`;
}

export function watchedLabel(n: number): string {
  return `Watched from ${n} other browser${n === 1 ? "" : "s"}`;
}

/** #870: the client → daemon frame a spectator sends to take control. The
 *  daemon ignores it from the pilot or a solo terminal. */
export const TAKE_CONTROL_FRAME = JSON.stringify({ type: "take_control" });

export const TAKE_CONTROL_LABEL = "Take control";

/** Window bus: a gesture outside the terminal (the toast of a refused send)
 *  asks the terminal of `session` open in this tab to take control. */
export const TAKE_CONTROL_REQUESTED = "pdo:take-control";

export function requestTakeControl(session: string): void {
  window.dispatchEvent(new CustomEvent<{ session: string }>(TAKE_CONTROL_REQUESTED, { detail: { session } }));
}

/** #870: shown briefly to a pilot that has just become a spectator. */
export const TAKEN_OVER_NOTICE = "Another browser took control";

/** How long the take-over notice stays up. */
export const TAKEN_OVER_NOTICE_MS = 4000;

/** #870: a pilot → spectator transition is a take-over by another browser (the
 *  pilot never loses the hand any other way). */
export function isTakenOver(prev: TerminalRole, next: TerminalRole): boolean {
  return prev.role === "pilot" && next.role === "spectator";
}
