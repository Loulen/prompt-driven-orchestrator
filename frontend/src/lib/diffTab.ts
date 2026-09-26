import type { RunState } from "../shared/generated/RunState";
import type { DiffFile } from "../shared/generated/DiffFile";

// Shared constants/helpers of the Diff tab (#748), kept out of the component
// file so Fast Refresh keeps working and tests can import them.

/** Past this many files the tab starts collapsed (a bandeau says so). */
export const LARGE_DIFF_FILES = 25;
/** Past this many patch lines a file is truncated behind "Show N more lines". */
export const TRUNCATE_LINES = 300;

/** Collapsed file paths; `null` = the user has not touched this Run's layout yet. */
export type CollapsedFiles = Set<string> | null;

/** The path a file is keyed on: its destination, else its source. */
export function fileKey(f: DiffFile): string {
  return f.new_path ?? f.old_path ?? "";
}

/**
 * The default layout of a freshly loaded diff: everything expanded (AC), except
 * a large diff, which starts collapsed, and binaries, which have no body.
 */
export function defaultCollapsed(files: DiffFile[]): Set<string> {
  const init = new Set<string>();
  const large = files.length > LARGE_DIFF_FILES;
  for (const f of files) {
    if (large || f.binary) init.add(fileKey(f));
  }
  return init;
}

/**
 * The signature of "what the Run has delivered so far": every node's latest
 * `delivery.after`. When it changes while the tab is open, the tip has moved
 * and the loaded diff is stale — the tab says so instead of re-rendering under
 * the reader's eyes (scroll and collapse state would be lost).
 */
export function deliverySignature(run: RunState): string {
  return Object.values(run.nodes)
    .map((n) => n.delivery?.after ?? "")
    .join("|");
}
