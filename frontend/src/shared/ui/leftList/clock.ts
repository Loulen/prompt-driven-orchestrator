import { useSyncExternalStore } from "react";

/**
 * The time the rows of a list count from: one clock for every row, read again
 * every `TICK_MS` while a row listens, so a duration and the time to a fire
 * move on their own without one timer per row.
 */
export const TICK_MS = 30_000;

let now = Date.now();
const listeners = new Set<() => void>();
let timer: ReturnType<typeof setInterval> | null = null;

function subscribe(listener: () => void) {
  listeners.add(listener);
  if (timer === null) {
    now = Date.now();
    timer = setInterval(() => {
      now = Date.now();
      for (const l of listeners) l();
    }, TICK_MS);
  }
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0 && timer !== null) {
      clearInterval(timer);
      timer = null;
    }
  };
}

/** The list's clock, in ms since the epoch. */
export function useListClock(): number {
  return useSyncExternalStore(subscribe, () => now);
}
