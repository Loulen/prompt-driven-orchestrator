// #867 / ADR-0075 — the role frames of the terminal socket.
//
// The daemon pushes `{"type":"role",…}` on the socket's Text channel at every
// change: `solo` (one poste: nothing shown), `pilot` (with the number of other
// postes watching) or `spectator` (with the pilot's grid).

import type { Dimensions } from "./altBufferResize";

/** #867 / ADR-0075: this terminal's role between postes, pushed by the daemon.
 *  `solo` is the default and shows nothing: one poste, or several tabs of it. */
export type TerminalRole =
  | { role: "solo" }
  | { role: "pilot"; spectators: number }
  | { role: "spectator"; pilot: Dimensions };

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
  if (m.role === "solo") return SOLO;
  if (m.role === "pilot") {
    const n = Number(m.spectators);
    return { role: "pilot", spectators: Number.isFinite(n) && n > 0 ? n : 0 };
  }
  if (m.role === "spectator") {
    const p = m.pilot as Record<string, unknown> | undefined;
    const cols = Number(p?.cols);
    const rows = Number(p?.rows);
    if (!(cols > 0) || !(rows > 0)) return null;
    return { role: "spectator", pilot: { cols, rows } };
  }
  return null;
}

export const READ_ONLY_HINT =
  "Read-only — another browser has control. Take control with the ✋ icon.";

export function watchedLabel(n: number): string {
  return `Watched from ${n} other browser${n === 1 ? "" : "s"}`;
}

/** #870: the client → daemon frame a spectator sends to take control. The
 *  daemon ignores it from the pilot or a solo terminal. */
export const TAKE_CONTROL_FRAME = JSON.stringify({ type: "take_control" });

export const TAKE_CONTROL_LABEL = "Take control";

/** #870: shown briefly to a pilot that has just become a spectator. */
export const TAKEN_OVER_NOTICE = "Another browser took control";

/** How long the take-over notice stays up. */
export const TAKEN_OVER_NOTICE_MS = 4000;

/** #870: a pilot → spectator transition is a take-over by another browser (the
 *  pilot never loses the hand any other way). */
export function isTakenOver(prev: TerminalRole, next: TerminalRole): boolean {
  return prev.role === "pilot" && next.role === "spectator";
}
