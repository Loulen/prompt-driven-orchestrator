/**
 * Sequential bulk executor for the multi-select action bar (#577). The daemon
 * exposes only per-item endpoints today (no batch route), so a bulk action is N
 * per-item calls. Deleting a run removes worktrees off disk with no undo, so the
 * loop is SEQUENTIAL — steady progress, one failure never aborts the rest, and
 * the daemon isn't hammered with a parallel burst of git/worktree work.
 *
 * `runBulk` NEVER rejects: a per-item rejection is captured as a failure so the
 * caller can report "10 done, 2 failed: …" (partial-failure is a first-class
 * outcome, not an exception).
 */
export interface BulkItem {
  id: string;
  /** Human label for the result summary (a run/trigger/pipeline name). */
  label: string;
}

export interface BulkItemResult extends BulkItem {
  ok: boolean;
  /** Present only on a failure — the per-item error message, surfaced verbatim. */
  error?: string;
}

export interface BulkOutcome {
  total: number;
  succeeded: BulkItemResult[];
  failed: BulkItemResult[];
}

export async function runBulk(
  items: BulkItem[],
  fn: (id: string) => Promise<void>,
  onProgress?: (done: number, total: number) => void,
): Promise<BulkOutcome> {
  const succeeded: BulkItemResult[] = [];
  const failed: BulkItemResult[] = [];
  const total = items.length;
  onProgress?.(0, total);
  let done = 0;
  for (const item of items) {
    try {
      await fn(item.id);
      succeeded.push({ ...item, ok: true });
    } catch (e) {
      failed.push({ ...item, ok: false, error: e instanceof Error ? e.message : String(e) });
    }
    done += 1;
    onProgress?.(done, total);
  }
  return { total, succeeded, failed };
}
