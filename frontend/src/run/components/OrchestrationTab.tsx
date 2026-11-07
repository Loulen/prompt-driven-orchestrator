// #723 — Orchestration tab of the NodeRun detail panel + the shared count pastilles.
import { useEffect, useState } from "react";
import { ArrowRight, ExternalLink, GitFork } from "lucide-react";
import { EmptyState, StatusIcon } from "../../shared/ui/kit";
import { isLiveRun } from "../model/runStatus";
import { formatDuration } from "../model/runDuration";
import { formatCostAmount } from "../model/costLabel";
import {
  childCostKnown,
  childCostText,
  type ChildCounts,
  type RunChildEntry,
  totalChildren,
} from "../model/orchestration";

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
  /** `pill`: a tinted pill per kind (canvas card, detail tab); `sm` / `xs`: a dot and a number. */
  size?: "sm" | "xs" | "pill";
  testId?: string;
  /** When set, the cluster is clickable and stops the click from reaching the row. */
  onClick?: () => void;
  /** Appended to every pill's tooltip when `onClick` is set (e.g. "click to expand"). */
  clickHint?: string;
}) {
  if (totalChildren(counts) === 0) return null;
  const pill = size === "pill";
  const dot = size === "xs" ? 6 : 7;
  const hint = onClick && clickHint ? ` — ${clickHint}` : "";
  const item = (n: number, bg: string, tone: string, label: string, id: string) =>
    n > 0 ? (
      pill ? (
        <span
          key={id}
          data-testid={`${testId}-${id}`}
          data-st={tone}
          title={`${n} ${label} child run${n > 1 ? "s" : ""}${hint}`}
          className="cv-kid"
        >
          <i aria-hidden />
          {n}
        </span>
      ) : (
        <span
          key={id}
          data-testid={`${testId}-${id}`}
          title={`${n} ${label} child run${n > 1 ? "s" : ""}${hint}`}
          className={`inline-flex items-center gap-1 font-mono leading-none text-text-2 ${size === "xs" ? "text-2xs" : "text-xs"}`}
        >
          <span className={`inline-block rounded-full ${bg}`} style={{ width: dot, height: dot }} />
          {n}
        </span>
      )
    ) : null;
  const label = `${counts.finished} finished, ${counts.failed} failed, ${counts.stale} stale, ${counts.running} running child runs`;
  const body = (
    <>
      {item(counts.finished, "bg-st-done", "done", "finished", "finished")}
      {item(counts.failed, "bg-st-failed", "failed", "failed", "failed")}
      {item(counts.stale, "bg-st-skipped", "blocked", "stale", "stale")}
      {item(counts.running, "bg-st-running", "running", "running", "running")}
    </>
  );
  if (onClick) {
    return (
      <span
        role="button"
        tabIndex={-1}
        className="inline-flex cursor-pointer items-center gap-2 rounded px-0.5 hover:bg-hover"
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
    <span className={pill ? "cv-kids" : "inline-flex items-center gap-2"} data-testid={testId} aria-label={label}>
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
  // #588: a child awaiting its user (itself or by derivation) is tinted the
  // awaiting tone, a failed one the failed tone: the row is the signpost.
  const awaiting = child.status === "awaiting_user";
  const name = child.name || child.run_id;
  return (
    <li
      data-testid="orchestration-child"
      data-run-id={child.run_id}
      data-awaiting={awaiting || undefined}
      data-st={child.status}
      className="ri-orch__row"
    >
      <StatusIcon kind="run" status={child.status} />
      <div className="ri-orch__stack">
        <div className="ri-orch__l1">
          <button
            type="button"
            data-testid="orchestration-child-title"
            onClick={() => onOpen(child.run_id)}
            title={`Open ${name}`}
            className="ri-orch__title"
          >
            <span>{name}</span>
            <ExternalLink size={11} strokeWidth={1.5} className="ri-orch__ext" aria-hidden />
          </button>
          {awaiting && (
            <button
              type="button"
              data-testid="orchestration-child-awaiting"
              className="ri-orch__you"
              onClick={() => onOpen(child.run_id)}
            >
              awaiting you
              <ArrowRight size={11} strokeWidth={1.5} aria-hidden />
            </button>
          )}
        </div>
        <div className="ri-orch__l2">
          <span>
            {child.pipeline_name}
            {started && ` · ${started.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`}
            {duration && (
              <span data-testid="orchestration-child-duration">
                {" · "}
                {duration}
                {live ? "…" : ""}
              </span>
            )}
          </span>
          <span data-testid="orchestration-child-cost" className="ri-orch__cost">
            {childCostText(child.cost)}
          </span>
        </div>
      </div>
    </li>
  );
}

export function OrchestrationTab({
  childRuns,
  nodeLive,
  onOpenChild,
}: {
  childRuns: RunChildEntry[];
  /** The parent node's session is still live (children may still appear). */
  nodeLive: boolean;
  onOpenChild: (runId: string) => void;
}) {
  if (childRuns.length === 0) {
    return (
      <div data-testid="orchestration-empty" title={nodeLive ? "The agent creates them with pdo run create" : undefined}>
        <EmptyState icon={GitFork} layout="row">
          {nodeLive ? "No child run yet" : "No child run"}
        </EmptyState>
      </div>
    );
  }
  return (
    <div className="ri-orch" data-testid="orchestration-tab">
      <ul className="m-0 list-none p-0">
        {childRuns.map((child) => (
          <ChildRow key={child.run_id} child={child} onOpen={onOpenChild} />
        ))}
      </ul>
      <div className="ri-orch__foot">
        <span>
          {childRuns.length} child run{childRuns.length > 1 ? "s" : ""}
        </span>
        <span className="ri-orch__fill" />
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
