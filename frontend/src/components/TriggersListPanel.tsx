import { Plus, Trash2, Zap } from "lucide-react";
import type { Trigger } from "../types";
import { deleteTrigger } from "../api";
import { humanizeCron } from "../cronPresets";

interface Props {
  triggers: Trigger[];
  selectedTriggerId: string | null;
  onSelectTrigger: (triggerId: string) => void;
  onNewTrigger: () => void;
  onTriggersChanged: () => void;
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
}: Props) {
  async function handleDelete(triggerId: string) {
    try {
      await deleteTrigger(triggerId);
      onTriggersChanged();
    } catch {
      // WS push / refresh will reconcile.
    }
  }

  return (
    <div className="flex h-full flex-col" data-testid="triggers-list-panel">
      <div
        className="flex h-[36px] shrink-0 items-center border-b border-line px-3 font-medium text-fg-2"
        style={{ fontSize: "11.5px" }}
      >
        Triggers
        <button
          onClick={onNewTrigger}
          className="ml-auto flex cursor-pointer items-center gap-1 rounded bg-acc px-1.5 py-0.5 font-medium text-[#04140d] transition-colors hover:bg-acc-dim"
          style={{ fontSize: "10.5px" }}
        >
          <Plus size={10} />
          New Trigger
        </button>
      </div>

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
          {triggers.map((t) => {
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
                        {t.target_repo.split("/").pop()}
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
                <span
                  role="button"
                  title="Delete trigger"
                  className="hidden shrink-0 cursor-pointer rounded p-0.5 text-fg-4 transition-colors hover:bg-bg-4 hover:text-st-failed group-hover:inline-flex"
                  onClick={(e) => {
                    e.stopPropagation();
                    void handleDelete(t.id);
                  }}
                  data-testid="trigger-delete"
                >
                  <Trash2 size={12} />
                </span>
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
