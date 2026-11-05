/**
 * At most one call of `fn` in flight (#1009). A call made while one is running
 * does not start a second request: it marks the result stale, and exactly one
 * more call runs once the current one settles — however many requests arrived
 * meanwhile. A burst of N triggers costs two requests, not N.
 *
 * Every trigger is honoured (the trailing call starts after the last trigger),
 * so the caller never misses an update; it only stops piling up requests the
 * daemon has not answered yet. That pile-up is what drove the daemon to the OOM
 * killer when it slowed down: every WebSocket event re-fetched `GET /runs`, and
 * the slower the answers, the more requests overlapped.
 */
export function singleFlight(fn: () => Promise<void>): () => Promise<void> {
  let running: Promise<void> | null = null;
  let again = false;

  const run = async (): Promise<void> => {
    try {
      do {
        again = false;
        try {
          await fn();
        } catch {
          /* the caller's own fn decides what a failure means; keep draining */
        }
      } while (again);
    } finally {
      running = null;
    }
  };

  return () => {
    if (running) {
      again = true;
      return running;
    }
    running = run();
    return running;
  };
}

/**
 * Poll `fn` every `intervalMs`, measured from the END of the previous call —
 * never two calls in flight, unlike `setInterval`, which fires on schedule
 * whether or not the daemon has answered (#1009). Returns the stop function.
 */
export function pollAfterResponse(fn: () => Promise<void>, intervalMs: number): () => void {
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const tick = async () => {
    try {
      await fn();
    } catch {
      /* the caller's own fn decides what a failure means */
    }
    if (!stopped) timer = setTimeout(tick, intervalMs);
  };

  void tick();
  return () => {
    stopped = true;
    if (timer !== null) clearTimeout(timer);
  };
}
