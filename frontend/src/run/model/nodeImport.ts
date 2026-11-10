/** « Fichier importé en cours de Run »: the pure rules of the import modal. */

import type { Actor } from "../../shared/generated/Actor";
import type { NodeStatus } from "../../shared/generated/NodeStatus";
import { ApiError } from "../../shared/http/client";

/** The phrase the modal hands the user for one imported file. The path is
 *  relative to the Run's worktree root. */
export function importedFilePhrase(path: string): string {
  return `I imported a file for you: \`${path}\` (path relative to the worktree root).`;
}

/** The ready-to-use text of an import: one phrase per file, joined by a space
 *  and never a line break, so writing it into a terminal never submits half of
 *  it. */
export function importedFilesText(paths: string[]): string {
  return paths.map(importedFilePhrase).join(" ");
}

/** Said beside « Copy & send to terminal » on a browser that is only watching
 *  the terminal (ADR-0075): writing into the agent's input is the pilot's, and
 *  the daemon refuses the send. */
export const SEND_NEEDS_CONTROL =
  "Another browser has control of the terminal: take control with the ✋ icon to send the text, or copy it.";

/** The daemon's refusal of a send from a spectator poste (`409 not_pilot`),
 *  with the pilot it names (`null` when it names none); `null` for any other
 *  failure. */
export function notPilotRefusal(error: unknown): { pilot: Actor | null } | null {
  if (!(error instanceof ApiError) || error.status !== 409) return null;
  const body = error.body as { error?: unknown; pilot?: Actor } | null | undefined;
  if (body?.error !== "not_pilot") return null;
  return { pilot: body.pilot ?? null };
}

/** Add `incoming` to the files staged for an import. A file with the same name
 *  replaces the staged one (the user picked it again). */
export function stageImportFiles(current: File[], incoming: ArrayLike<File>): File[] {
  let next = [...current];
  for (const file of Array.from(incoming)) {
    const dup = next.findIndex((f) => f.name === file.name);
    next = dup >= 0 ? next.map((f, i) => (i === dup ? file : f)) : [...next, file];
  }
  return next;
}

/** Why the node on screen cannot receive files right now, or `null` when it
 *  can: only the current iteration of a node that is running or waiting for
 *  the user holds a live session. The daemon refuses the same cases anyway. */
export function importBlockedReason(args: {
  isArchived: boolean;
  selectedIter: number;
  currentIter: number;
  status: NodeStatus;
}): string | null {
  const { isArchived, selectedIter, currentIter, status } = args;
  if (isArchived) return "This Run is archived: its nodes have no session to receive files.";
  if (selectedIter !== currentIter) {
    return `Only the current iteration (iter ${currentIter}) has a live session: switch to it to import files.`;
  }
  if (status !== "running" && status !== "awaiting_user") {
    return "This node has no live session: files can only be imported into a node that is running.";
  }
  return null;
}
