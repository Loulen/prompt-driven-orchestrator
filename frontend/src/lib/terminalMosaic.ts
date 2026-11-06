/**
 * #1040 — the « Mosaïque »: the Terminal agrandi split into panes, up to four,
 * to follow several sessions at once (see CONTEXT.md « Mosaïque »).
 *
 * The pane rules live here, free of React: the composition (the node's own
 * terminal first, then what was added), the dedupe, the cap, the layout, and
 * the « last mosaic » this poste can take back. The view only renders them.
 *
 * - The **primary** pane is the node terminal the view was expanded from. It is
 *   never in the list of added panes and never leaves the view.
 * - Adding a session already shown **focuses** its pane instead of opening it
 *   twice: two panes on one session would fight over its tmux size.
 * - The **last mosaic** belongs to the poste (this browser's `localStorage`, like
 *   `pdo.poste`): one person's composition never shows up for another. It is the
 *   last composition that had at least two panes — shrinking back to the lone
 *   terminal does not erase it, so it can still be taken back.
 */

import type { PaneSource } from "../components/TmuxTerminal";
import { terminalTitle } from "./terminalIdentity";

/** Four panes: two side by side, three in columns, four in a 2×2 grid. */
export const MAX_PANES = 4;

export type TerminalKind = "node" | "manager" | "shell";

/**
 * A terminal one can show in a pane: which tmux session, and what it is, in the
 * words of its identity (#1038). A node carries its `paneSource` so a pane whose
 * session ends can read the frozen snapshot (#617).
 */
export interface TerminalRef {
  session: string;
  kind: TerminalKind;
  runId: string;
  runName: string | null;
  /** The node's name, « Manager » or « Shell ». */
  subject: string;
  paneSource?: PaneSource;
}

/** « Run › subject », the identity every pane shows. */
export function paneTitle(ref: TerminalRef): string {
  return terminalTitle(ref.runName, ref.subject);
}

export interface AddOutcome {
  /** The added panes after the gesture (unchanged when nothing was added). */
  extras: TerminalRef[];
  /** The session whose pane gets the keyboard, or `null` when refused. */
  focus: string | null;
  added: boolean;
}

/**
 * Adds `ref` as a pane. A session already shown (the primary included) is not
 * opened again: its pane is focused. Past {@link MAX_PANES} nothing is added.
 */
export function addPane(primarySession: string, extras: TerminalRef[], ref: TerminalRef): AddOutcome {
  if (ref.session === primarySession || extras.some((p) => p.session === ref.session)) {
    return { extras, focus: ref.session, added: false };
  }
  if (1 + extras.length >= MAX_PANES) return { extras, focus: null, added: false };
  return { extras: [...extras, ref], focus: ref.session, added: true };
}

/** The ✕ of a pane: out of the view. Nothing is stopped — the session runs on. */
export function removePane(extras: TerminalRef[], session: string): TerminalRef[] {
  return extras.filter((p) => p.session !== session);
}

/** Can one more pane join? */
export function canAddPane(extras: TerminalRef[]): boolean {
  return 1 + extras.length < MAX_PANES;
}

/**
 * The rows of the grid, the primary always first in the first row: one row up
 * to three panes (side by side, then columns), two rows of two at four.
 */
export function mosaicRows<T>(panes: T[]): T[][] {
  if (panes.length <= 3) return [panes];
  return [panes.slice(0, 2), panes.slice(2, MAX_PANES)];
}

// --- the last mosaic, per poste ---------------------------------------------

export const LAST_MOSAIC_KEY = "pdo.mosaic.last";

function defaultStorage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

function isPaneSource(v: unknown): v is PaneSource {
  if (typeof v !== "object" || v === null) return false;
  const o = v as Record<string, unknown>;
  return typeof o.runId === "string" && typeof o.nodeId === "string" && typeof o.iter === "number";
}

function isTerminalRef(v: unknown): v is TerminalRef {
  if (typeof v !== "object" || v === null) return false;
  const o = v as Record<string, unknown>;
  return (
    typeof o.session === "string" &&
    o.session !== "" &&
    (o.kind === "node" || o.kind === "manager" || o.kind === "shell") &&
    typeof o.runId === "string" &&
    (o.runName === null || typeof o.runName === "string") &&
    typeof o.subject === "string" &&
    (o.paneSource === undefined || isPaneSource(o.paneSource))
  );
}

/**
 * Remembers `panes` (primary first) as this poste's last mosaic. A lone
 * terminal is no mosaic: it leaves the previous one in place. Storage
 * unavailable (private mode, quota) ⇒ nothing remembered, never a throw.
 */
export function saveLastMosaic(panes: TerminalRef[], storage: Storage | null = defaultStorage()): void {
  if (panes.length < 2 || !storage) return;
  try {
    storage.setItem(LAST_MOSAIC_KEY, JSON.stringify(panes.slice(0, MAX_PANES)));
  } catch {
    // Not remembered: the mosaic itself still works.
  }
}

/** This poste's last mosaic, or `null` (none, unreadable, corrupt). */
export function loadLastMosaic(storage: Storage | null = defaultStorage()): TerminalRef[] | null {
  if (!storage) return null;
  try {
    const raw = storage.getItem(LAST_MOSAIC_KEY);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return null;
    const panes = parsed.filter(isTerminalRef);
    return panes.length >= 2 ? panes : null;
  } catch {
    return null;
  }
}

/** Does the last mosaic hold anything besides the terminal already shown? */
export function hasRestorable(primarySession: string, last: TerminalRef[] | null): boolean {
  return last !== null && last.some((p) => p.session !== primarySession);
}

/**
 * The added panes « Restore last mosaic » gives back around `primarySession`:
 * the last composition's sessions that are **still live**, in their order,
 * without the primary, capped. A live entry's fresh identity wins over the
 * remembered one (a Run renamed since).
 */
export function restoreMosaic(
  primarySession: string,
  last: TerminalRef[] | null,
  live: TerminalRef[],
): TerminalRef[] {
  if (!last) return [];
  const liveBySession = new Map(live.map((r) => [r.session, r]));
  let extras: TerminalRef[] = [];
  for (const pane of last) {
    const fresh = liveBySession.get(pane.session);
    if (!fresh) continue;
    extras = addPane(primarySession, extras, fresh).extras;
  }
  return extras;
}
