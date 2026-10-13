import { useEffect, useState } from "react";
import type { NodeStatus } from "../types";

/** The timing of one iteration of a node: what its Total duration is read from. */
export interface IterationTiming {
  status: NodeStatus;
  started_at: string | null;
  completed_at: string | null;
}

/**
 * An iteration still under way (#979): its duration keeps growing, it gets the
 * state dot, and its tooltip says `running` instead of an end time.
 */
export function iterationIsLive(status: NodeStatus): boolean {
  return status === "running" || status === "awaiting_user";
}

/**
 * Total duration (Mode de durée Total, `NodeStarted` -> end) of an iteration, in
 * milliseconds. `now` stands in for the end while the iteration is live. `null`
 * when the iteration never started, or when it stopped without an end time we
 * could read (no honest figure to show).
 */
export function iterationTotalMs(timing: IterationTiming, now: number): number | null {
  if (!timing.started_at) return null;
  const start = Date.parse(timing.started_at);
  if (Number.isNaN(start)) return null;
  let end: number;
  if (timing.completed_at) end = Date.parse(timing.completed_at);
  else if (iterationIsLive(timing.status)) end = now;
  else return null;
  if (Number.isNaN(end)) return null;
  return Math.max(0, end - start);
}

/**
 * `Date.now()`, refreshed every second while `ticking`. Modeled on
 * `useRunDuration`: no interval at all once the iteration is over. The value is
 * read at mount, so a caller switching iterations remounts (`key`) to start fresh.
 */
export function useNow(ticking: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!ticking) return;
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [ticking]);
  return now;
}

/** Live Total duration of an iteration: ticks while it runs, frozen at its end. */
export function useIterationTotalMs(timing: IterationTiming): number | null {
  const ticking =
    !!timing.started_at && timing.completed_at == null && iterationIsLive(timing.status);
  const now = useNow(ticking);
  return iterationTotalMs(timing, now);
}
