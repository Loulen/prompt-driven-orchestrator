/**
 * #1040 — a poll that never stacks: the next round is scheduled only once the
 * previous one has settled, so at most one round is ever in flight. A plain
 * `setInterval` keeps firing while a slow daemon has not answered, and the
 * requests pile up on it (the OOM of #1009).
 *
 * - The first round runs at once; each next one `intervalMs` after the previous
 *   one settled (fulfilled or rejected — a failure waits for the next round).
 * - A round due while the tab is hidden is skipped, not run: nobody is looking.
 * - A round resolving to `false` ends the poll.
 *
 * Returns the stop function (an effect's cleanup).
 */
export function pollAfterResponse(
  round: () => Promise<boolean | void>,
  intervalMs: number,
  isHidden: () => boolean = () => document.hidden,
): () => void {
  let stopped = false;
  let timer: number | undefined;

  const schedule = () => {
    if (!stopped) timer = window.setTimeout(() => void run(), intervalMs);
  };

  const run = async () => {
    if (stopped) return;
    if (isHidden()) {
      schedule();
      return;
    }
    let again: boolean | void = true;
    try {
      again = await round();
    } catch {
      // Try again at the next round.
    }
    if (again === false) {
      stopped = true;
      return;
    }
    schedule();
  };

  void run();
  return () => {
    stopped = true;
    window.clearTimeout(timer);
  };
}
