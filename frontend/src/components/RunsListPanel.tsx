import type { RunListEntry, RunStatus } from "../types";

const STATUS_STYLES: Record<RunStatus, { dot: string; bg: string }> = {
  running: { dot: "bg-st-running", bg: "bg-st-running-bg" },
  completed: { dot: "bg-st-done", bg: "bg-st-done-bg" },
  failed: { dot: "bg-st-failed", bg: "bg-st-failed-bg" },
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

          return (
            <button
              key={run.run_id}
              onClick={() => onSelectRun(run.run_id)}
              className={`flex w-full items-center gap-2 border-b border-line-soft px-3 py-2 text-left transition-colors ${
                isSelected
                  ? "bg-bg-3 text-fg"
                  : "text-fg-2 hover:bg-bg-3/50"
              }`}
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
            </button>
          );
        })}
      </div>
    </aside>
  );
}
