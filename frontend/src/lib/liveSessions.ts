/**
 * #1040 — the live terminals « + terminal » offers: every session one can show
 * in a pane of the Mosaïque, from any Run.
 *
 * The daemon has no list of tmux sessions (`GET /sessions` answers counts), so
 * the list is read off the Runs: each live Run's detail names its nodes whose
 * current iteration is running or awaiting the user, and says whether its
 * Manager session exists (`has_manager`, probed in tmux at fetch time). A Run's
 * Shell is not listed: nothing on the wire says whether it is open, and opening
 * it to find out would create it.
 */

import { isLiveRun, type NodeStatus, type RunListEntry, type RunState } from "../types";
import type { TerminalRef } from "./terminalMosaic";

/** A node iteration in these states has its session up. */
const LIVE_NODE_STATUSES: ReadonlySet<NodeStatus> = new Set(["running", "awaiting_user"]);

export function nodeSessionName(runId: string, nodeId: string, iter: number): string {
  return `pdo-${runId}-${nodeId}-iter-${iter}`;
}

export function managerSessionName(runId: string): string {
  return `pdo-mgr-${runId}`;
}

export function shellSessionName(runId: string): string {
  return `pdo-shell-${runId}`;
}

/** The live terminals of one Run: its Manager first, then its nodes, in graph order. */
export function liveSessionsOfRun(run: RunState): TerminalRef[] {
  const runName = run.name?.trim() || null;
  const refs: TerminalRef[] = [];
  if (run.has_manager) {
    refs.push({
      session: managerSessionName(run.run_id),
      kind: "manager",
      runId: run.run_id,
      runName,
      subject: "Manager",
    });
  }
  const order = new Map((run.node_defs ?? []).map((d, i) => [d.id, i]));
  const nodes = Object.values(run.nodes ?? {})
    .filter((n) => LIVE_NODE_STATUSES.has(n.status))
    .sort(
      (a, b) =>
        (order.get(a.node_id) ?? Number.MAX_SAFE_INTEGER) -
          (order.get(b.node_id) ?? Number.MAX_SAFE_INTEGER) || a.node_id.localeCompare(b.node_id),
    );
  for (const node of nodes) {
    const name = run.node_defs?.find((d) => d.id === node.node_id)?.name;
    refs.push({
      session: nodeSessionName(run.run_id, node.node_id, node.iter),
      kind: "node",
      runId: run.run_id,
      runName,
      subject: name?.trim() || node.node_id,
      paneSource: { runId: run.run_id, nodeId: node.node_id, iter: node.iter },
    });
  }
  return refs;
}

export interface LiveSessionSources {
  fetchRuns: () => Promise<RunListEntry[]>;
  fetchRun: (runId: string) => Promise<RunState>;
}

/**
 * Every live terminal of every live Run. A Run whose detail cannot be read is
 * left out rather than failing the whole list; the list itself failing throws.
 */
export async function collectLiveSessions(sources: LiveSessionSources): Promise<TerminalRef[]> {
  const runs = (await sources.fetchRuns()).filter((r) => isLiveRun(r.status));
  const details = await Promise.allSettled(runs.map((r) => sources.fetchRun(r.run_id)));
  return details.flatMap((d) => (d.status === "fulfilled" ? liveSessionsOfRun(d.value) : []));
}

/**
 * The search of the picker: every word of the query must appear in the Run's
 * name or the subject (node name, « Manager »), case- and accent-insensitive.
 */
export function searchSessions(refs: TerminalRef[], query: string): TerminalRef[] {
  const words = fold(query).split(/\s+/).filter(Boolean);
  if (words.length === 0) return refs;
  return refs.filter((r) => {
    const haystack = fold(`${r.runName ?? ""} ${r.subject} ${r.paneSource?.nodeId ?? ""}`);
    return words.every((w) => haystack.includes(w));
  });
}

function fold(s: string): string {
  return s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

/**
 * An iteration in these states has had its session reaped and its pane frozen:
 * its status will not change again. The daemon's `iter_is_terminal` set
 * (`interrupted` is not in it: its session may still be alive).
 */
const TERMINAL_ITER_STATUSES: ReadonlySet<string> = new Set(["completed", "failed", "stopped", "stale"]);

export function isTerminalIterStatus(status: string | undefined): boolean {
  return status !== undefined && TERMINAL_ITER_STATUSES.has(status);
}

/**
 * The status of the iteration a node pane shows, read off its Run: what lets the
 * pane know its session ended (and show the frozen snapshot, not a dropped
 * connection). `undefined` when the Run does not know the node.
 */
export function paneIterStatus(run: RunState, nodeId: string, iter: number): NodeStatus | undefined {
  const node = run.nodes?.[nodeId];
  if (!node) return undefined;
  const it = node.iterations?.find((i) => i.iter === iter);
  if (it) return it.status;
  return node.iter === iter ? node.status : undefined;
}
