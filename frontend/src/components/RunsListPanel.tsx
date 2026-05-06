import { useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import type { RunListEntry, RunStatus } from "../types";
import { cleanupRun } from "../api";
import CleanupConfirmModal from "./CleanupConfirmModal";

const STATUS_STYLES: Record<RunStatus, { dot: string; bg: string }> = {
  running: { dot: "bg-st-running", bg: "bg-st-running-bg" },
  awaiting_user: { dot: "bg-st-await", bg: "bg-st-await-bg" },
  completed: { dot: "bg-st-done", bg: "bg-st-done-bg" },
  failed: { dot: "bg-st-failed", bg: "bg-st-failed-bg" },
  halted: { dot: "bg-st-blocked", bg: "bg-st-blocked-bg" },
  archived: { dot: "bg-st-archived", bg: "bg-st-archived-bg" },
};

interface Props {
  runs: RunListEntry[];
  selectedRunId: string | null;
  onSelectRun: (runId: string) => void;
  onNewRun: () => void;
}

export default function RunsListPanel({
  runs,
  selectedRunId,
  onSelectRun,
  onNewRun,
}: Props) {
  const [confirmCleanup, setConfirmCleanup] = useState<string | null>(null);

  async function handleCleanup(runId: string) {
    try {
      await cleanupRun(runId);
    } catch {
      // event-driven refresh will pick up state change
    }
    setConfirmCleanup(null);
  }

  return (
    <aside className="flex h-full flex-col bg-bg-2">
      <div
        className="flex h-[36px] items-center border-b border-line px-3 font-medium text-fg-2"
        style={{ fontSize: "11.5px" }}
      >
        Runs
        <button
          onClick={onNewRun}
          className="ml-auto flex items-center gap-1 rounded bg-acc px-1.5 py-0.5 font-medium text-[#04140d] transition-colors hover:bg-acc-dim"
          style={{ fontSize: "10.5px" }}
        >
          <Plus size={10} />
          New Run
        </button>
      </div>
      <div className="flex-1 overflow-y-auto">
        {runs.length === 0 && (
          <div
            className="px-3 py-4 text-center text-fg-4"
            style={{ fontSize: "11px" }}
          >
            No runs yet
          </div>
        )}
        {runs.map((run) => {
          const isSelected = run.run_id === selectedRunId;
          const { dot } = STATUS_STYLES[run.status] ?? STATUS_STYLES.running;
          const isTerminal = run.status === "completed"
            || run.status === "failed"
            || run.status === "halted";

          return (
            <button
              key={run.run_id}
              onClick={() => onSelectRun(run.run_id)}
              className={`group flex w-full items-center gap-2 border-b border-line-soft px-3 py-2 text-left transition-colors ${
                isSelected
                  ? "bg-bg-3 text-fg"
                  : "text-fg-2 hover:bg-bg-3/50"
              } ${run.status === "archived" ? "opacity-60" : ""}`}
              style={{ fontSize: "11.5px" }}
            >
              <span
                className={`h-2 w-2 shrink-0 rounded-full ${dot} ${
                  run.status === "running" ? "animate-pulse" : ""
                }`}
              />
              <div className="min-w-0 flex-1">
                <div className="truncate font-medium">{run.pipeline_name}</div>
                <div
                  className="truncate font-mono text-fg-4"
                  style={{ fontSize: "10px" }}
                >
                  {run.run_id.slice(0, 20)}
                </div>
              </div>
              {isTerminal && (
                <span
                  role="button"
                  title="Cleanup run"
                  className="hidden shrink-0 rounded p-0.5 text-fg-4 transition-colors hover:bg-bg-4 hover:text-fg-2 group-hover:inline-flex"
                  onClick={(e) => {
                    e.stopPropagation();
                    setConfirmCleanup(run.run_id);
                  }}
                >
                  <Trash2 size={12} />
                </span>
              )}
            </button>
          );
        })}
      </div>

      {confirmCleanup && (
        <CleanupConfirmModal
          onConfirm={() => handleCleanup(confirmCleanup)}
          onCancel={() => setConfirmCleanup(null)}
        />
      )}
    </aside>
  );
}
