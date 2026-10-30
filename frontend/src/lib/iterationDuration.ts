import { useEffect, useState } from "react";
import type { NodeStatus } from "../types";

/** The timing of one iteration of a node: what its durations are read from. */
export interface IterationTiming {
  status: NodeStatus;
  started_at: string | null;
  completed_at: string | null;
  /**
   * #980: the iteration's **closed** Attentes déclarées, in ms, as the daemon
   * projects them (`IterationInfo.waited_ms`, the rule Stats -> Performance
   * uses). Absent on an older daemon: no closed wait.
   */
  waited_ms?: number;
  /** #980: when the declared wait still open on this iteration began, if any. */
  open_wait_since?: string | null;
}

/** The three Modes de durée of one iteration, in ms: Total = Active + Waiting. */
export interface IterationDurations {
  total: number;
  active: number;
  waiting: number;
}

/**
 * Whether an `awaiting.cause` is an Attente déclarée, the same rule as the
 * daemon's: `declared`, `completion_not_released` and `turn_ended` (an
 * interactive node's turn handed back, ADR-0081) are; `children_pending` (an
 * orchestrator at work on its children) and `child_awaiting` are not.
 */
export function isDeclaredWaitCause(cause: string | null | undefined): boolean {
  return cause === "declared" || cause === "completion_not_released" || cause === "turn_ended";
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
 * Total, Active and Waiting of an iteration at `now` (#980). Waiting = the
 * closed waits (`waited_ms`) + the wait still open (`now - open_wait_since`);
 * Active = Total - Waiting. While a wait is open, Total keeps growing and Active
 * stays put: both grow with `now`, so their difference does not. Waiting is
 * capped at Total (a clock skew must not make Active negative, nor break
 * Total = Active + Waiting). `null` when there is no Total.
 */
export function iterationDurations(
  timing: IterationTiming,
  now: number,
): IterationDurations | null {
  const total = iterationTotalMs(timing, now);
  if (total == null) return null;
  let waiting = Math.max(0, timing.waited_ms ?? 0);
  if (timing.open_wait_since) {
    const since = Date.parse(timing.open_wait_since);
    if (!Number.isNaN(since)) waiting += Math.max(0, now - since);
  }
  waiting = Math.min(waiting, total);
  return { total, active: total - waiting, waiting };
}

/**
 * The durations as the panel prints them (#980 FP): whole seconds, so that
 * Total = Active + Waiting holds on screen too, not only in ms. Each figure
 * would otherwise lose its own fraction (`3m 18s` != `1m 2s` + `2m 15s`).
 *
 * Seconds are rounded, as Stats -> Performance rounds them, so a finished
 * iteration reads the same in both places. The figure that does not move is
 * rounded on its own and the third one is derived: during an open wait Active
 * is frozen (rounded, Waiting = Total - Active), otherwise Waiting is (rounded,
 * Active = Total - Waiting). Deriving the frozen figure instead would make it
 * flicker by a second as the other two tick.
 */
export function displayedDurations(
  durations: IterationDurations,
  waitOpen: boolean,
): IterationDurations {
  const seconds = (ms: number) => Math.round(ms / 1000) * 1000;
  const total = seconds(durations.total);
  if (waitOpen) {
    const active = Math.min(seconds(durations.active), total);
    return { total, active, waiting: total - active };
  }
  const waiting = Math.min(seconds(durations.waiting), total);
  return { total, active: total - waiting, waiting };
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

/**
 * Live durations of an iteration, as displayed (`displayedDurations`): tick
 * every second while it runs (an open wait included: Total keeps going, Active
 * is frozen), fixed once it ends.
 */
export function useIterationDurations(timing: IterationTiming): IterationDurations | null {
  const ticking =
    !!timing.started_at && timing.completed_at == null && iterationIsLive(timing.status);
  const now = useNow(ticking);
  const durations = iterationDurations(timing, now);
  return durations && displayedDurations(durations, !!timing.open_wait_since);
}
