import { useEffect, useSyncExternalStore } from "react";
import { request } from "../../shared/http/client";
import type { RunCostsResponse } from "../../shared/generated/RunCostsResponse";
import type { RunStatus } from "../../shared/generated/RunStatus";
import { isLiveRun } from "../model/runStatus";

/**
 * The cost the rows of the Runs list show (`GET /runs/costs`, ADR-0052:
 * derived on read, apart from `GET /runs`). A row asks for its Run; the asks
 * of one frame go in one read. An ended Run is read once; a live one again
 * after `LIVE_TTL_MS`.
 */
export const LIVE_TTL_MS = 60_000;
/** The daemon prices at most this many Runs per read. */
const BATCH = 100;

interface Entry {
  /** `null`: no source tells it. */
  usd: number | null;
  /** The status it was read under: a Run that ends is read again. */
  live: boolean;
  at: number;
}

const byRun = new Map<string, Entry>();
const listeners = new Set<() => void>();
const wanted = new Map<string, boolean>();
const inFlight = new Set<string>();
let scheduled = false;

function changed() {
  for (const listener of listeners) listener();
}

function stale(runId: string, live: boolean, now: number): boolean {
  const entry = byRun.get(runId);
  if (!entry) return true;
  if (entry.live && !live) return true;
  return live && now - entry.at > LIVE_TTL_MS;
}

async function flush() {
  scheduled = false;
  const asks = [...wanted.entries()];
  wanted.clear();
  for (let i = 0; i < asks.length; i += BATCH) {
    const batch = asks.slice(i, i + BATCH);
    const ids = batch.map(([id]) => id);
    ids.forEach((id) => inFlight.add(id));
    try {
      const answer = await request<RunCostsResponse>("GET", "/runs/costs", { query: { ids: ids.join(",") } });
      const at = Date.now();
      const costs = new Map(answer.runs.map((r) => [r.run_id, r.cost]));
      for (const [id, live] of batch) {
        // A cost a harness cannot sum honestly is shown as nothing, never `$0`.
        const cost = costs.get(id);
        const usd = cost && cost.uncosted_harnesses.length === 0 ? cost.usd : null;
        byRun.set(id, { usd, live, at });
      }
    } catch {
      // Nothing shown; the next ask reads again.
    } finally {
      ids.forEach((id) => inFlight.delete(id));
    }
  }
  changed();
}

/** Ask for `runId`'s cost, read with the other asks of this frame. */
export function wantRunCost(runId: string, status: RunStatus) {
  const live = isLiveRun(status);
  if (inFlight.has(runId) || !stale(runId, live, Date.now())) return;
  wanted.set(runId, live);
  if (scheduled) return;
  scheduled = true;
  queueMicrotask(() => void flush());
}

/**
 * The cost of `runId` in dollars; `null` while unknown or when none is told.
 * A `null` Run (a sample) asks nothing.
 */
export function useRunCost(runId: string | null, status: RunStatus): number | null {
  const usd = useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => (runId === null ? null : (byRun.get(runId)?.usd ?? null)),
  );
  useEffect(() => {
    if (runId === null) return;
    wantRunCost(runId, status);
    if (!isLiveRun(status)) return;
    const timer = window.setInterval(() => wantRunCost(runId, status), LIVE_TTL_MS);
    return () => window.clearInterval(timer);
  }, [runId, status]);
  return usd;
}

/** Tests: forget every cost read. */
export function resetRunCosts() {
  byRun.clear();
  wanted.clear();
  inFlight.clear();
  scheduled = false;
}
