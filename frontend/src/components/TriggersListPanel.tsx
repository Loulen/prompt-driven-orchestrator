import { AlertCircle, PauseCircle, Pencil, Play, Plus, Power, Trash2, Zap } from "lucide-react";
import type { Trigger } from "../types";
import { deleteTrigger, updateTrigger } from "../api";
import { humanizeCron } from "../cronPresets";
import { groupByRepo, repoGroupLabel } from "../lib/groupByRepo";

interface Props {
  triggers: Trigger[];
  selectedTriggerId: string | null;
  onSelectTrigger: (triggerId: string) => void;
  onNewTrigger: () => void;
  onTriggersChanged: () => void;
  /** Open the New Run modal pre-filled from this Trigger (Run-now mode). */
  onRunNow?: (trigger: Trigger) => void;
  /** Open the modal in edit mode for this Trigger. */
  onEditTrigger?: (trigger: Trigger) => void;
  /** #348 global kill-switch: whether all scheduled fires are currently paused. */
  paused?: boolean;
  /** #348: flip the global pause flag. */
  onTogglePause?: () => void;
}

/**
 * Map a Trigger's `last_outcome` to a status-dot color (reuses the run status
 * tokens). No outcome yet ⇒ a neutral "pending" dot.
 */
function outcomeDot(outcome: string | null | undefined): string {
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

function lastOutcomeTooltip(t: Trigger): string {
  if (!t.last_outcome) return "never fired";
  const when = t.last_fired_at ?? "—";
  return `last run: ${when}, result: ${t.last_outcome}`;
}

export default function TriggersListPanel({
  triggers,
  selectedTriggerId,
  onSelectTrigger,
  onNewTrigger,
  onTriggersChanged,
  onRunNow,
  onEditTrigger,
  paused = false,
  onTogglePause,
}: Props) {
  async function handleDelete(triggerId: string) {
    try {
      await deleteTrigger(triggerId);
      onTriggersChanged();
    } catch {
      // WS push / refresh will reconcile.
    }
  }

  async function handleToggle(t: Trigger) {
    try {
      await updateTrigger(t.id, { enabled: !t.enabled });
      onTriggersChanged();
    } catch {
      // WS push / refresh will reconcile.
    }
  }

  // Raw target repos actually set on rows — drives the per-row badge label so a
  // colliding basename is disambiguated identically in the badge and the group
  // header (#258 G7). Computed once per render.
  const allRepos = triggers
    .map((t) => t.target_repo)
    .filter((r): r is string => !!r);

  // One trigger row, rendered identically flat or grouped by repo (#258).
  function renderTriggerRow(t: Trigger) {
    const isSelected = t.id === selectedTriggerId;
    const dot = outcomeDot(t.last_outcome);
    return (
      <button
        key={t.id}
        onClick={() => onSelectTrigger(t.id)}
        className={`group flex w-full cursor-pointer items-center gap-2 border-b border-line-soft px-3 py-2 text-left transition-colors ${
          isSelected ? "bg-bg-3 text-fg" : "text-fg-2 hover:bg-bg-3/50"
        } ${t.enabled ? "" : "opacity-60"}`}
        style={{ fontSize: "11.5px" }}
        data-testid="trigger-row"
      >
        <span
          className={`h-2 w-2 shrink-0 rounded-full ${dot}`}
          title={lastOutcomeTooltip(t)}
          data-testid="trigger-status-dot"
        />
        <div className="min-w-0 flex-1">
          <div className="truncate font-medium">{t.name}</div>
          <div
            className="mt-0.5 flex items-center gap-1.5 truncate text-fg-4"
            style={{ fontSize: "10px" }}
          >
            <span className="truncate font-mono">{t.pipeline_name}</span>
            {t.target_repo && (
              <span
                className="shrink-0 rounded border border-line-strong px-1 py-px"
                style={{ fontSize: "9px" }}
                title={t.target_repo}
              >
                {repoGroupLabel(t.target_repo, allRepos)}
              </span>
            )}
            <span className="shrink-0" aria-hidden="true">·</span>
            <span className="shrink-0" data-testid="trigger-schedule">
              {humanizeCron(t.cron)}
            </span>
          </div>
        </div>
        {!t.enabled && (
          <span
            className="shrink-0 rounded border border-line-strong px-1 py-px text-fg-4"
            style={{ fontSize: "9px" }}
          >
            disabled
          </span>
        )}
        {/* Hover actions: run-now, edit, delete. */}
        <span className="hidden shrink-0 items-center gap-0.5 group-hover:inline-flex">
          <span
            role="button"
            title="Run now (test this trigger)"
            className="cursor-pointer rounded p-0.5 text-fg-4 transition-colors hover:bg-bg-4 hover:text-acc"
            onClick={(e) => {
              e.stopPropagation();
              onRunNow?.(t);
            }}
            data-testid="trigger-run-now"
          >
            <Play size={12} />
          </span>
          <span
            role="button"
            title="Edit trigger"
            className="cursor-pointer rounded p-0.5 text-fg-4 transition-colors hover:bg-bg-4 hover:text-fg"
            onClick={(e) => {
              e.stopPropagation();
              onEditTrigger?.(t);
            }}
            data-testid="trigger-edit"
          >
            <Pencil size={12} />
          </span>
          <span
            role="button"
            title="Delete trigger"
            className="cursor-pointer rounded p-0.5 text-fg-4 transition-colors hover:bg-bg-4 hover:text-st-failed"
            onClick={(e) => {
              e.stopPropagation();
              void handleDelete(t.id);
            }}
            data-testid="trigger-delete"
          >
            <Trash2 size={12} />
          </span>
        </span>
        {/* Enable/disable toggle — always visible so paused state is reversible. */}
        <span
          role="switch"
          aria-checked={t.enabled}
          aria-label={t.enabled ? "Disable trigger" : "Enable trigger"}
          title={t.enabled ? "Disable trigger" : "Enable trigger"}
          className={`shrink-0 cursor-pointer rounded p-0.5 transition-colors hover:bg-bg-4 ${
            t.enabled ? "text-acc" : "text-fg-4"
          }`}
          onClick={(e) => {
            e.stopPropagation();
            void handleToggle(t);
          }}
          data-testid="trigger-toggle"
        >
          <Power size={12} />
        </span>
      </button>
    );
  }

  // Group the Triggers list by project (#258) only when ≥ 2 distinct repos are
  // present; otherwise `null` ⇒ the flat list, byte-identical to before.
  const triggerGroups = groupByRepo(triggers, (t) => t.effective_repo);

  return (
    <div className="flex h-full flex-col" data-testid="triggers-list-panel">
      <div
        className="flex h-[36px] shrink-0 items-center border-b border-line px-3 font-medium text-fg-2"
        style={{ fontSize: "11.5px" }}
      >
        Triggers
        <div className="ml-auto flex items-center gap-2">
          {/* #348 master switch — global kill-switch, deliberately distinct from
              the per-row enable/disable toggle (amber st-await when active, vs the
              per-row green/grey Power). */}
          <span
            role="switch"
            aria-checked={paused}
            aria-label={paused ? "Resume all triggers" : "Pause all triggers"}
            title={
              paused
                ? "Resume all triggers"
                : "Pause all triggers (global kill-switch)"
            }
            onClick={onTogglePause}
            data-testid="triggers-pause-switch"
            className={`flex cursor-pointer items-center gap-1 rounded px-1.5 py-0.5 transition-colors ${
              paused
                ? "bg-st-await-bg text-st-await"
                : "text-fg-4 hover:bg-bg-4 hover:text-fg-2"
            }`}
            style={{ fontSize: "10.5px" }}
          >
            <PauseCircle size={12} />
            {paused ? "Paused" : "Pause all"}
          </span>
          <button
            onClick={onNewTrigger}
            className="flex cursor-pointer items-center gap-1 rounded bg-acc px-1.5 py-0.5 font-medium text-[#04140d] transition-colors hover:bg-acc-dim"
            style={{ fontSize: "10.5px" }}
          >
            <Plus size={10} />
            New Trigger
          </button>
        </div>
      </div>

      {/* #348 amber banner — the two channels (global pause vs per-row disabled)
          stay visually distinct: this suppression banner never grays a row. */}
      {paused && (
        <div
          className="flex items-center gap-2 border-b border-st-await/30 bg-st-await-bg px-3 py-2"
          data-testid="triggers-paused-banner"
        >
          <AlertCircle size={14} className="shrink-0 text-st-await" />
          <span
            className="text-st-await"
            style={{ fontSize: "11.5px", fontWeight: 500 }}
          >
            All triggers paused — scheduled fires are suppressed. Manual “Run now”
            still works.
          </span>
        </div>
      )}

      {triggers.length === 0 ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 px-6 text-center">
          <Zap size={22} className="text-fg-4" />
          <div className="text-fg-3" style={{ fontSize: "12px" }}>
            No triggers yet
          </div>
          <div className="text-fg-4" style={{ fontSize: "10.5px", lineHeight: 1.4 }}>
            Triggers start a Run on a schedule, so recurring work happens without
            you launching it by hand.
          </div>
          <button
            onClick={onNewTrigger}
            className="mt-1 flex cursor-pointer items-center gap-1 rounded bg-acc px-2 py-1 font-medium text-[#04140d] transition-colors hover:bg-acc-dim"
            style={{ fontSize: "10.5px" }}
          >
            <Plus size={11} />
            New Trigger
          </button>
        </div>
      ) : (
        <div className="flex-1 overflow-y-auto">
          {triggerGroups === null
            ? triggers.map(renderTriggerRow)
            : triggerGroups.map((group) => (
                <div key={group.repoPath} data-testid="trigger-repo-group">
                  <div
                    className="flex h-[22px] shrink-0 items-center border-b border-line-soft bg-bg-3/40 px-3 font-medium text-fg-3"
                    style={{ fontSize: "10px" }}
                    title={group.repoPath}
                  >
                    <span className="truncate" data-testid="trigger-repo-label">
                      {group.label}
                    </span>
                  </div>
                  {group.items.map(renderTriggerRow)}
                </div>
              ))}
        </div>
      )}
    </div>
  );
}
