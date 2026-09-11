// #723 — Orchestration read model shared by the Orchestration tab, the tab-header
// pastilles and the canvas pastilles, so the three surfaces can never disagree.
import { useEffect, useState } from "react";
import type { HarnessCost, RunStatus } from "../types";
import { isLiveRun } from "../types";
import { formatEstCost } from "./costLabel";

/** One child of `GET /runs/{id}/children` (daemon `RunChildEntry`). */
export interface RunChildEntry {
  run_id: string;
  pipeline_name: string;
  name?: string;
  status: RunStatus;
  /**
   * #783 — display-only "no forward progress" overlay, derived per read by the
   * daemon exactly as on `GET /runs` (#180). Optional so a daemon predating the
   * field (or a fixture omitting it) still typechecks — absent reads as not stalled.
   */
  stalled?: boolean;
  started_at?: string;
  completed_at?: string;
  /** The daemon's CostStat, derived on read (ADR-0052) — never persisted.
   *  Shape mirrors `RunState.cost` (no `form` at the top level: the per-slice
   *  forms live in `by_harness`). */
  cost?: {
    usd: number;
    partial: boolean;
    unpriced_models?: string[];
    uncosted_harnesses?: string[];
    by_harness?: HarnessCost[];
  };
}

export interface RunChildrenGroup {
  node_id: string | null;
  children: RunChildEntry[];
}

export interface RunChildrenResponse {
  run_id: string;
  nodes: RunChildrenGroup[];
}

/**
 * Pastille counters — the « compteurs d'enfants » of CONTEXT.md (#783): green
 * finished / red failed / orange stale / blue running. Four DISJOINT sets whose
 * sum is the number of children (or descendants, on the run list).
 */
export interface ChildCounts {
  /** Terminal, not failed: completed, halted, skipped, archived. */
  finished: number;
  failed: number;
  /** Live AND `stalled` (#180): no forward progress, amber like the list dot. */
  stale: number;
  /** Live and not stalled: running, awaiting_user, paused. */
  running: number;
}

/**
 * `awaiting_user` / `paused` count as **running** (decision 3 of the design,
 * 2026-09-07): the blue pill is "still to settle", and a failed child is red.
 * #783 carves **stale** out of running (live + `stalled`) — the four sets stay
 * disjoint, so the pills always total the children.
 */
export function countChildren(children: RunChildEntry[]): ChildCounts {
  const counts: ChildCounts = { finished: 0, failed: 0, stale: 0, running: 0 };
  for (const child of children) {
    if (child.status === "failed") counts.failed += 1;
    else if (isLiveRun(child.status)) {
      if (child.stalled) counts.stale += 1;
      else counts.running += 1;
    } else counts.finished += 1;
  }
  return counts;
}

export function totalChildren(counts: ChildCounts): number {
  return counts.finished + counts.failed + counts.stale + counts.running;
}

/**
 * The child's cost, rendered through the shared honesty vocabulary (#272/#377):
 * "—" when unavailable, never "$0". Unavailable means: the cost is absent,
 * a harness has no cost source (the helper itself refuses to sum), or there
 * was simply no costable session (`usd == 0` and no per-harness slice).
 */
export function childCostText(cost?: RunChildEntry["cost"]): string {
  if (!cost) return "—";
  const byHarness = cost.by_harness ?? [];
  if ((cost.uncosted_harnesses?.length ?? 0) > 0) {
    return formatEstCost(cost.usd, cost.partial, cost.unpriced_models ?? [], cost.uncosted_harnesses ?? [], byHarness).text;
  }
  if (cost.usd === 0 && byHarness.length === 0) return "—";
  return formatEstCost(cost.usd, cost.partial, cost.unpriced_models ?? [], [], byHarness).text;
}

/** The child's summable dollars, or `null` when its cost is unavailable (the
 *  footer's Σ marks itself partial on any `null`). */
export function childCostKnown(cost?: RunChildEntry["cost"]): number | null {
  if (!cost) return null;
  if ((cost.uncosted_harnesses?.length ?? 0) > 0) return null;
  if (cost.usd === 0 && (cost.by_harness ?? []).length === 0) return null;
  return cost.usd;
}

/**
 * Poll `GET /runs/{id}/children` at the same cadence as the run view refresh
 * (never pushed — design §3). Non-2xx and network errors keep the last good
 * read: a momentarily unreachable daemon must not blank a surface the user is
 * reading. The response is NEVER reset to null between runs — `childrenOfNode`
 * guards on the response's own `run_id`, so a stale read can never dress
 * another run's cards (no setState-in-effect reset to trip the compiler).
 */
export function useRunChildren(runId: string | null, enabled: boolean): RunChildrenResponse | null {
  const [data, setData] = useState<RunChildrenResponse | null>(null);
  useEffect(() => {
    if (!runId || !enabled) return;
    let cancelled = false;
    const load = async () => {
      try {
        const res = await fetch(`/runs/${runId}/children`);
        if (!res.ok) return;
        const json = (await res.json()) as RunChildrenResponse;
        if (!cancelled) setData(json);
      } catch {
        /* keep the last good read */
      }
    };
    void load();
    const id = setInterval(load, 3000);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [runId, enabled]);
  return data;
}

/** This node's slice of the children response, in the daemon's (chronological)
 *  order — empty unless the read belongs to THIS run (the hook never resets,
 *  so the consumer must run-guard). */
export function childrenOfNode(
  data: RunChildrenResponse | null,
  runId: string,
  nodeId: string,
): RunChildEntry[] {
  if (!data || data.run_id !== runId) return [];
  return data.nodes.find((g) => g.node_id === nodeId)?.children ?? [];
}
