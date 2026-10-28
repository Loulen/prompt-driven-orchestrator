import { Zap } from "lucide-react";
import type { NodeProgress } from "../../../shared/generated/NodeProgress";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import { StatusIcon, Tooltip } from "../../../shared/ui/kit";
import type { StatusTone } from "../../../shared/ui/kit";
import { fmtCost, fmtSpan, spanOf } from "../../../shared/ui/leftList/format";
import { useListClock } from "../../../shared/ui/leftList/clock";
import type { ChildCounts } from "../../model/orchestration";
import { isLiveRun } from "../../model/runStatus";
import { useRunCost } from "../../store/runCosts";

/** The most node squares a row draws; the rest is a count. */
const MAX_PIPS = 8;

/** The progress of a Run: one square per working node, the live one pulsing, an awaiting one hollow. */
export function NodePips({ nodes }: { nodes: NodeProgress[] }) {
  if (nodes.length === 0) return null;
  const done = nodes.filter((n) => n.status === "completed" || n.status === "skipped").length;
  const shown = nodes.slice(0, MAX_PIPS);
  const label = `${done} of ${nodes.length} nodes done`;
  return (
    <Tooltip label={label}>
      <span className="lp-pips" role="img" aria-label={label} data-testid="run-node-pips">
        {shown.map((n) => (
          <i key={n.id} data-st={n.status} data-node={n.name ?? n.id} aria-hidden="true" />
        ))}
        {nodes.length > MAX_PIPS && <span className="lp-pips__more">+{nodes.length - MAX_PIPS}</span>}
      </span>
    </Tooltip>
  );
}

const PILLS: { key: keyof ChildCounts; tone: StatusTone; word: string }[] = [
  { key: "running", tone: "running", word: "running" },
  { key: "failed", tone: "failed", word: "failed" },
  { key: "stale", tone: "skipped", word: "stale" },
  { key: "finished", tone: "done", word: "finished" },
];

/**
 * The counts of a parent's descendants (#783), an icon and a number each, zero
 * ones left out. On a folded parent they unfold it.
 */
export function ChildPills({ counts, onExpand }: { counts: ChildCounts; onExpand?: () => void }) {
  const shown = PILLS.filter((p) => counts[p.key] > 0);
  if (shown.length === 0) return null;
  const label = `${counts.finished} finished, ${counts.failed} failed, ${counts.stale} stale, ${counts.running} running child runs`;
  return (
    <span
      className="lp-pills"
      role={onExpand ? "button" : undefined}
      tabIndex={onExpand ? -1 : undefined}
      aria-label={onExpand ? `${label}, expand` : label}
      data-testid="run-child-pills"
      onClick={
        onExpand &&
        ((e) => {
          e.stopPropagation();
          onExpand();
        })
      }
    >
      {shown.map((p) => {
        const n = counts[p.key];
        const title = `${n} ${p.word} child run${n > 1 ? "s" : ""}${onExpand ? ", click to expand" : ""}`;
        return (
          <span key={p.key} className="lp-pill" title={title} data-testid={`run-child-pills-${p.key}`}>
            <StatusIcon kind="tone" status={p.tone} label={p.word} size={11} title={title} />
            {n}
          </span>
        );
      })}
    </span>
  );
}

/** The Trigger that created the Run, as a bolt: the click opens it in the Triggers tab. */
export function TriggerMark({ name, onOpen }: { name: string; onOpen: () => void }) {
  const label = `Trigger: ${name}`;
  return (
    <Tooltip label={label}>
      <button
        type="button"
        className="lp-trig"
        aria-label={label}
        data-testid="run-trigger-badge"
        onClick={(e) => {
          e.stopPropagation();
          onOpen();
        }}
        onKeyDown={(e) => e.stopPropagation()}
      >
        <Zap size={11} strokeWidth={1.75} aria-hidden />
      </button>
    </Tooltip>
  );
}

/** Where the duration of a Run ends: its end, its last activity while paused, now while it runs. */
function endOf(run: RunListEntry): string | null {
  if (run.status === "running" || run.status === "awaiting_user") return null;
  return run.completed_at ?? run.last_activity_at ?? null;
}

/** Its duration and its cost, at the end of line 2. */
export function RunFigures({ run }: { run: RunListEntry }) {
  const usd = useRunCost(run.run_id, run.status);
  const now = useListClock();
  const ms = spanOf(run.started_at, endOf(run), now);
  const started = run.started_at ? `Started ${new Date(run.started_at).toLocaleString()}` : undefined;
  return (
    <>
      {ms !== null && (
        <span
          className="lp-num"
          title={[isLiveRun(run.status) ? "Elapsed" : "Duration", started].filter(Boolean).join("\n")}
          data-testid="run-duration"
        >
          {fmtSpan(ms)}
        </span>
      )}
      {usd !== null && (
        <span className="lp-num lp-muted" title="Cost (estimate)" data-testid="run-cost">
          {fmtCost(usd)}
        </span>
      )}
    </>
  );
}
