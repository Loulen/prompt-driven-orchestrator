// #723 — Orchestration tab of the NodeRun detail panel + the shared count pastilles.
import { useEffect, useState } from "react";
import { ExternalLink, GitFork } from "lucide-react";
import type { RunStatus } from "../types";
import { isLiveRun } from "../types";
import { formatDuration } from "../lib/runDuration";
import { formatCostAmount } from "../lib/costLabel";
import {
  childCostKnown,
  childCostText,
  type ChildCounts,
  type RunChildEntry,
  totalChildren,
} from "../lib/orchestration";

const RUN_STATUS_LABEL: Record<RunStatus, string> = {
  running: "Running",
  awaiting_user: "Awaiting user",
  completed: "Completed",
  failed: "Failed",
  skipped: "Skipped",
  halted: "Halted",
  paused: "Paused",
  archived: "Archived",
};

function runStatusDot(status: RunStatus): string {
  if (status === "failed") return "bg-st-failed";
  if (status === "awaiting_user") return "bg-st-await";
  if (status === "paused") return "bg-st-stopped";
  if (isLiveRun(status)) return "bg-st-running";
  return "bg-st-done";
}

/**
 * The green / red / orange / blue counters (« compteurs d'enfants », #723/#783):
 * a coloured dot with the number beside it. ONE component for the canvas node,
 * the tab header and the run list's parent rows, so the surfaces can never
 * disagree; a zero counter is omitted, and the whole cluster is omitted when
 * there is no child at all (nodes that never orchestrated stay clean).
 *
 * `onClick` (run list) makes the cluster a button — clicking the pills of a
 * collapsed parent expands it (design decision 1, 2026-09-11); the tooltip then
 * says so. `subject` names what is counted in the tooltip ("child run" on a
 * node, "descendant" would be wrong there).
 */
export function ChildCountPills({
  counts,
  size = "sm",
  testId = "child-count-pills",
  onClick,
  clickHint,
}: {
  counts: ChildCounts;
  size?: "sm" | "xs";
  testId?: string;
  /** When set, the cluster is clickable and stops the click from reaching the row. */
  onClick?: () => void;
  /** Appended to every pill's tooltip when `onClick` is set (e.g. "click to expand"). */
  clickHint?: string;
}) {
  if (totalChildren(counts) === 0) return null;
  const dot = size === "xs" ? 6 : 7;
  const fs = size === "xs" ? 9.5 : 10.5;
  const hint = onClick && clickHint ? ` — ${clickHint}` : "";
  const item = (n: number, bg: string, label: string, id: string) =>
    n > 0 ? (
      <span
        key={id}
        data-testid={`${testId}-${id}`}
        title={`${n} ${label} child run${n > 1 ? "s" : ""}${hint}`}
        className="inline-flex items-center gap-1 font-mono text-fg-2"
        style={{ fontSize: fs, lineHeight: 1 }}
      >
        <span className={`inline-block rounded-full ${bg}`} style={{ width: dot, height: dot }} />
        {n}
      </span>
    ) : null;
  const label = `${counts.finished} finished, ${counts.failed} failed, ${counts.stale} stale, ${counts.running} running child runs`;
  const body = (
    <>
      {item(counts.finished, "bg-st-done", "finished", "finished")}
      {item(counts.failed, "bg-st-failed", "failed", "failed")}
      {item(counts.stale, "bg-st-stale", "stale", "stale")}
      {item(counts.running, "bg-st-running", "running", "running")}
    </>
  );
  if (onClick) {
    return (
      <span
        role="button"
        tabIndex={-1}
        className="inline-flex cursor-pointer items-center gap-2 rounded px-0.5 hover:bg-bg-4"
        data-testid={testId}
        aria-label={label}
        onClick={(e) => {
          e.stopPropagation();
          onClick();
        }}
      >
        {body}
      </span>
    );
  }
  return (
    <span className="inline-flex items-center gap-2" data-testid={testId} aria-label={label}>
      {body}
    </span>
  );
}

/**
 * The tick a live child's duration counts against. `Date.now()` is impure and
 * may not be called during render — the clock lives in a timer callback
 * instead, and `null` (before the first tick) renders a bare « … ».
 */
function useNow(live: boolean): number | null {
  const [now, setNow] = useState<number | null>(null);
  useEffect(() => {
    if (!live) return;
    const tick = () => setNow(Date.now());
    const id = setInterval(tick, 1000);
    return () => clearInterval(id);
  }, [live]);
  return live ? now : null;
}

function ChildRow({
  child,
  onOpen,
}: {
  child: RunChildEntry;
  onOpen: (runId: string) => void;
}) {
  const live = isLiveRun(child.status);
  const now = useNow(live);
  const started = child.started_at ? new Date(child.started_at) : null;
  const ended = child.completed_at
    ? new Date(child.completed_at).getTime()
    : now;
  const duration =
    started && ended != null ? formatDuration(ended - started.getTime()) : null;
  const failed = child.status === "failed";
  return (
    <li
      data-testid="orchestration-child"
      data-run-id={child.run_id}
      className={`group flex flex-col gap-0.5 rounded border px-2 py-1.5 ${
        failed ? "border-st-failed/40 bg-st-failed-bg/40" : "border-line bg-bg-3"
      }`}
    >
      <div className="flex items-center gap-1.5">
        <span
          title={RUN_STATUS_LABEL[child.status] ?? child.status}
          className={`h-1.5 w-1.5 shrink-0 rounded-full ${runStatusDot(child.status)} ${live ? "animate-pulse" : ""}`}
        />
        <button
          type="button"
          data-testid="orchestration-child-title"
          onClick={() => onOpen(child.run_id)}
          title="Open this child run — the back arrow brings you back here"
          className="flex min-w-0 flex-1 cursor-pointer items-center gap-1 text-left font-medium text-fg hover:text-acc hover:underline"
          style={{ fontSize: "11.5px" }}
        >
          <span className="truncate">{child.name || child.run_id}</span>
          <ExternalLink size={10} className="shrink-0 opacity-0 transition-opacity group-hover:opacity-100" />
        </button>
      </div>
      <div className="flex items-center gap-1 pl-3 font-mono text-fg-4" style={{ fontSize: "9.5px" }}>
        <span className="truncate">{child.pipeline_name}</span>
        {started && (
          <span>
            {" · "}
            started {started.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
          </span>
        )}
        {duration && (
          <span data-testid="orchestration-child-duration">
            {" · "}
            {duration}
            {live ? "…" : ""}
          </span>
        )}
        <span data-testid="orchestration-child-cost" className="ml-auto">
          {childCostText(child.cost)}
        </span>
      </div>
    </li>
  );
}

export function OrchestrationTab({
  childRuns,
  counts,
  nodeLive,
  nodeAwaiting,
  onOpenChild,
}: {
  childRuns: RunChildEntry[];
  counts: ChildCounts;
  /** The orchestrator node's session is still live (children may still appear). */
  nodeLive: boolean;
  /** The node is `awaiting_user` — with a failed child, the tab says why. */
  nodeAwaiting: boolean;
  onOpenChild: (runId: string) => void;
}) {
  if (childRuns.length === 0) {
    return (
      <div
        data-testid="orchestration-empty"
        className="flex flex-1 flex-col items-center justify-center gap-1.5 px-6 py-8 text-center text-fg-4"
        style={{ fontSize: "11px" }}
      >
        <GitFork size={16} className="text-fg-5" />
        {nodeLive ? (
          <>
            <span className="text-fg-3">No child run yet.</span>
            <span>
              The agent creates them with <code className="font-mono text-fg-3">pdo run create</code>; they
              will appear here as they start.
            </span>
          </>
        ) : (
          <span>This node created no child run.</span>
        )}
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-1.5 px-3 py-2" data-testid="orchestration-tab">
      {nodeAwaiting && counts.failed > 0 && (
        <div
          data-testid="orchestration-failed-banner"
          className="rounded border border-st-failed/40 bg-st-failed-bg px-2 py-1.5 text-st-failed"
          style={{ fontSize: "10.5px", lineHeight: 1.45 }}
        >
          {counts.failed} child run{counts.failed > 1 ? "s" : ""} failed — the node waits for you. Open the
          child to retry it, or <em>Mark complete</em> above to force the node through.
        </div>
      )}
      {nodeLive && counts.running > 0 && (
        <div className="flex items-center gap-1.5 text-fg-4" style={{ fontSize: "10px" }}>
          <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-st-running" />
          Node completes once every child is terminal.
        </div>
      )}
      <ul className="flex flex-col gap-1">
        {childRuns.map((child) => (
          <ChildRow key={child.run_id} child={child} onOpen={onOpenChild} />
        ))}
      </ul>
      <div className="flex items-center justify-between font-mono text-fg-4" style={{ fontSize: "9.5px" }}>
        <span>
          {childRuns.length} child run{childRuns.length > 1 ? "s" : ""}
        </span>
        <span title="Sum of the children's estimated costs">
          Σ{" "}
          {(() => {
            const known = childRuns.map((c) => childCostKnown(c.cost));
            const summable = known.filter((v): v is number => v != null);
            if (summable.length === 0) return "—";
            const sum = summable.reduce((acc, v) => acc + v, 0);
            const partial = summable.length < childRuns.length;
            return formatCostAmount(sum, partial, true);
          })()}
        </span>
      </div>
    </div>
  );
}
