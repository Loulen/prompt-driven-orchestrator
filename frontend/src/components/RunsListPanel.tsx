import { useState } from "react";
import { Trash2 } from "lucide-react";
import type { RunListEntry, RunStatus } from "../types";
import { cleanupRun } from "../api";

const STATUS_STYLES: Record<RunStatus, { dot: string }> = {
  running: { dot: "bg-st-running" },
  completed: { dot: "bg-st-done" },
  failed: { dot: "bg-st-failed" },
  archived: { dot: "bg-st-archived" },
};

interface Props {
  runs: RunListEntry[];
  selectedRunId: string | null;
  onSelectRun: (runId: string) => void;
}

export default function RunsListPanel({
  runs,
  selectedRunId,
  onSelectRun,
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
    <aside className="flex w-[220px] shrink-0 flex-col border-r border-line bg-bg-2">
      <div
        className="flex h-[36px] items-center border-b border-line px-3 font-medium text-fg-2"
        style={{ fontSize: "11.5px" }}
      >
        Runs
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
          const isTerminal = run.status === "completed" || run.status === "failed";

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

      {/* Cleanup confirmation modal */}
      {confirmCleanup && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div className="w-[360px] rounded-lg border border-line bg-bg-2 p-4 shadow-lg">
            <h3 className="font-medium text-fg" style={{ fontSize: "13px" }}>
              Cleanup Run
            </h3>
            <p className="mt-2 text-fg-3" style={{ fontSize: "12px" }}>
              This will remove worktrees and artifacts. Event history is kept. Proceed?
            </p>
            <div className="mt-4 flex justify-end gap-2">
              <button
                onClick={() => setConfirmCleanup(null)}
                className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
                style={{ fontSize: "11.5px" }}
              >
                Cancel
              </button>
              <button
                onClick={() => handleCleanup(confirmCleanup)}
                className="rounded-md bg-st-failed px-3 py-1.5 text-white transition-colors hover:bg-st-failed/80"
                style={{ fontSize: "11.5px" }}
              >
                Cleanup
              </button>
            </div>
          </div>
        </div>
      )}
    </aside>
  );
}
