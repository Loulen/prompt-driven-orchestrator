import { useEffect, useState } from "react";
import { Clock, FolderGit2, GitBranch, History, Layers, Shield, Zap } from "lucide-react";
import type { Trigger, TriggerFire } from "../types";
import { fetchTriggerFires } from "../api";
import { humanizeCron } from "../cronPresets";

interface Props {
  trigger: Trigger;
  /** Jump to the Run a "fired" history entry created. */
  onSelectRun: (runId: string) => void;
}

/**
 * Right-panel detail view for a selected Trigger (#162). Shows the full config
 * (read-only — editing goes through the New Run modal) plus a reverse-chrono
 * fire history: the answer to "why didn't it fire last night?". Each "fired"
 * entry links to the Run it created.
 */
export default function TriggerDetailPanel({ trigger, onSelectRun }: Props) {
  const [fires, setFires] = useState<TriggerFire[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    // Bounded cascade: this effect only re-runs when `trigger.id` changes, so
    // resetting the loading flag here does not loop.
    // eslint-disable-next-line react-hooks/set-state-in-effect -- see note above.
    setLoading(true);
    fetchTriggerFires(trigger.id)
      .then((rows) => {
        if (!cancelled) setFires(rows);
      })
      .catch(() => {
        if (!cancelled) setFires([]);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [trigger.id]);

  return (
    <aside className="flex h-full flex-col overflow-y-auto bg-bg-2" data-testid="trigger-detail-panel">
      {/* Header */}
      <div className="flex items-center gap-2 border-b border-line px-3 py-2">
        <Zap size={14} className="shrink-0 text-acc" />
        <div className="min-w-0">
          <div className="truncate font-medium text-fg" style={{ fontSize: "12.5px" }}>
            {trigger.name}
          </div>
          <div className="mt-0.5 flex items-center gap-1.5 text-fg-4" style={{ fontSize: "10px" }}>
            <span>trigger</span>
            {!trigger.enabled && (
              <span
                className="rounded border border-line-strong px-1 py-px"
                style={{ fontSize: "9px" }}
                data-testid="trigger-detail-disabled"
              >
                disabled
              </span>
            )}
          </div>
        </div>
      </div>

      {/* Configuration */}
      <div className="flex flex-col gap-2 p-3" style={{ fontSize: "11.5px" }}>
        <span
          className="font-medium uppercase tracking-wider text-fg-4"
          style={{ fontSize: "10px" }}
        >
          Configuration
        </span>

        <ConfigRow icon={<Layers size={12} />} label="Pipeline">
          <span className="font-mono text-fg-2">{trigger.pipeline_name || trigger.pipeline_id}</span>
        </ConfigRow>

        {trigger.target_repo && (
          <ConfigRow icon={<FolderGit2 size={12} />} label="Repo">
            <span className="truncate font-mono text-fg-2" title={trigger.target_repo}>
              {trigger.target_repo}
            </span>
          </ConfigRow>
        )}

        {trigger.source_branch && (
          <ConfigRow icon={<GitBranch size={12} />} label="Branch">
            <span className="font-mono text-fg-2">{trigger.source_branch}</span>
          </ConfigRow>
        )}

        <ConfigRow icon={<Clock size={12} />} label="Schedule">
          <span className="text-fg-2" data-testid="trigger-detail-schedule">
            {humanizeCron(trigger.cron)}
          </span>
        </ConfigRow>

        <ConfigRow icon={<Layers size={12} />} label="Overlap">
          <span className="text-fg-2" data-testid="trigger-detail-overlap">
            {trigger.overlap_policy === "allow" ? "allow (concurrent fires)" : "skip (default)"}
          </span>
        </ConfigRow>

        {trigger.guard_command && (
          <div className="flex flex-col gap-1">
            <div className="flex items-center gap-1.5 text-fg-3">
              <Shield size={12} />
              <span>Guard</span>
            </div>
            <pre
              className="overflow-x-auto rounded border border-line bg-bg-3 px-2 py-1.5 font-mono text-fg-2"
              style={{ fontSize: "10.5px" }}
              data-testid="trigger-detail-guard"
            >
              {trigger.guard_command}
            </pre>
          </div>
        )}

        {trigger.input_template && (
          <div className="flex flex-col gap-1">
            <div className="text-fg-3">Input template</div>
            <pre
              className="whitespace-pre-wrap rounded border border-line bg-bg-3 px-2 py-1.5 font-mono text-fg-2"
              style={{ fontSize: "10.5px" }}
              data-testid="trigger-detail-input"
            >
              {trigger.input_template}
            </pre>
          </div>
        )}

        <ConfigRow icon={<Clock size={12} />} label="Next fire">
          <span className="font-mono text-fg-2">{formatTs(trigger.next_fire_at) ?? "—"}</span>
        </ConfigRow>
      </div>

      {/* Fire history */}
      <div className="flex flex-col gap-2 border-t border-line p-3" style={{ fontSize: "11.5px" }}>
        <div className="flex items-center gap-1.5">
          <History size={12} className="text-fg-3" />
          <span
            className="font-medium uppercase tracking-wider text-fg-4"
            style={{ fontSize: "10px" }}
          >
            Fire history
          </span>
        </div>

        {loading ? (
          <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
            Loading…
          </div>
        ) : fires.length === 0 ? (
          <div className="text-fg-4" style={{ fontSize: "10.5px" }} data-testid="fire-history-empty">
            No fires yet — entries appear here each time the trigger evaluates.
          </div>
        ) : (
          <div className="flex flex-col gap-1.5" data-testid="fire-history">
            {fires.map((f) => (
              <FireEntry key={f.id} fire={f} onSelectRun={onSelectRun} />
            ))}
          </div>
        )}
      </div>
    </aside>
  );
}

function ConfigRow({
  icon,
  label,
  children,
}: {
  icon: React.ReactNode;
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center gap-2">
      <span className="flex w-20 shrink-0 items-center gap-1.5 text-fg-3">
        {icon}
        {label}
      </span>
      <div className="min-w-0 flex-1 truncate">{children}</div>
    </div>
  );
}

/** Outcome → status-dot color (reuses the run status tokens, like the list). */
function outcomeDot(outcome: string): string {
  switch (outcome) {
    case "fired":
      return "bg-st-done";
    case "error":
      return "bg-st-failed";
    case "skipped-overlap":
    case "guard-exit-nonzero":
      return "bg-st-paused";
    case "guard-error":
      return "bg-st-blocked";
    default:
      return "bg-st-archived";
  }
}

function FireEntry({
  fire,
  onSelectRun,
}: {
  fire: TriggerFire;
  onSelectRun: (runId: string) => void;
}) {
  return (
    <div
      className="flex flex-col gap-0.5 rounded border border-line bg-bg-3 px-2 py-1.5"
      data-testid="fire-entry"
    >
      <div className="flex items-center gap-1.5">
        <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${outcomeDot(fire.outcome)}`} />
        <span className="font-medium text-fg-2">{fire.outcome}</span>
        <span className="ml-auto font-mono text-fg-4" style={{ fontSize: "10px" }}>
          {formatTs(fire.ts)}
        </span>
      </div>
      {fire.reason && (
        <div className="text-fg-3" style={{ fontSize: "10.5px" }}>
          {fire.reason}
        </div>
      )}
      {fire.run_id && (
        <button
          onClick={() => onSelectRun(fire.run_id!)}
          className="self-start font-mono text-acc hover:underline"
          style={{ fontSize: "10.5px" }}
          data-testid="fire-run-link"
          title="Open the run this fire created"
        >
          {fire.run_id}
        </button>
      )}
    </div>
  );
}

function formatTs(iso: string | null | undefined): string | null {
  if (!iso) return null;
  try {
    return new Date(iso).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return iso;
  }
}
