/** « Fichier importé en cours de Run »: the pure rules of the import modal. */

import type { NodeStatus } from "../../shared/generated/NodeStatus";

/** The ready-to-use text the modal hands the user once the files landed in the
 *  node's Blackboard: one line, so writing it into a terminal never submits half
 *  of it. Paths are relative to the Run's worktree root. */
export function importedFilesText(paths: string[]): string {
  const quoted = paths.map((p) => `\`${p}\``).join(", ");
  if (paths.length === 1) {
    return `I imported a file for you: ${quoted} (path relative to the worktree root).`;
  }
  return `I imported ${paths.length} files for you: ${quoted} (paths relative to the worktree root).`;
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
