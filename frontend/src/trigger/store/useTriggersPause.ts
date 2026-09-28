import { useCallback, useState } from "react";
import { fetchTriggersHealth, pauseTriggers } from "../api/triggers";

/**
 * #348: the global Trigger pause. Held by the app shell (not a per-panel hook)
 * so the WS dispatcher can flip it live (`setPaused`) and every consumer stays in
 * sync; `hydrate` reads it once from `GET /triggers/health`, since there is no
 * trigger polling to carry it.
 */
export function useTriggersPause() {
  const [paused, setPaused] = useState(false);

  // Optimistic (the master switch reflects the intent immediately); the daemon's
  // WS broadcast re-affirms it for every client, and a failure reverts so a dead
  // switch can't mislead the operator.
  const toggle = useCallback(async () => {
    const next = !paused;
    setPaused(next);
    try {
      await pauseTriggers(next);
    } catch {
      setPaused(!next);
    }
  }, [paused]);

  const hydrate = useCallback(() => {
    fetchTriggersHealth()
      .then((h) => setPaused(h.paused))
      .catch(() => {});
  }, []);

  return { paused, setPaused, toggle, hydrate };
}
