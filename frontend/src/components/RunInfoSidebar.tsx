import type { RunState } from "../types";

/**
 * Right-panel header shown when a run-scoped edit tab is open with nothing
 * selected on the canvas. For a live/completed run it notes that edits sync
 * back to the template; for an archived run (#315) the canvas is read-only
 * (its worktree + `pipeline.yaml` are gone and Save is disabled), so it says
 * so instead of the misleading "changes sync to template" note.
 */
export default function RunInfoSidebar({ run }: { run: RunState }) {
  const archived = run.status === "archived";
  return (
    <aside className="flex h-full flex-col bg-bg-2" style={{ fontSize: "12px" }}>
      <div className="border-b border-line px-3 py-3">
        <div className="font-medium text-fg">{run.pipeline_name}</div>
        <div className="mt-0.5 font-mono text-fg-4" style={{ fontSize: "10px" }}>
          {run.run_id}
        </div>
        <div
          className="mt-2 rounded border border-line-strong bg-bg-3 px-2 py-1.5 text-fg-3"
          style={{ fontSize: "10.5px" }}
          data-testid="run-info-note"
        >
          {archived
            ? "Archived run · read-only · outputs preserved"
            : "Editing run-scoped pipeline · changes sync to template"}
        </div>
      </div>
    </aside>
  );
}
