import { useState } from "react";
import { Star, Info, Terminal, X } from "lucide-react";
import { Tooltip } from "./ui/tooltip";
import { SectionHead } from "./InspectorPrimitives";
import TmuxTerminal from "./TmuxTerminal";
import { saveLibraryPipeline, deleteLibraryPipeline } from "../api";
import type { LibraryPipelineEntry } from "../api";
import type { RunState, PipelineDef } from "../types";
import { serializePipeline } from "../stores/editStore";

interface Props {
  run: RunState | null;
  pipeline: PipelineDef | null;
  libraryPipelines: LibraryPipelineEntry[];
  onLibraryChanged: () => void;
  onClose: () => void;
}

const STATUS_DOT: Record<string, string> = {
  running: "bg-st-running animate-pulse",
  awaiting_user: "bg-st-await",
  completed: "bg-st-done",
  failed: "bg-st-failed",
  halted: "bg-st-blocked",
  archived: "bg-st-archived",
  pending: "bg-st-pending",
};

export default function PipelineInfoPanel({
  run,
  pipeline,
  libraryPipelines,
  onLibraryChanged,
  onClose,
}: Props) {
  const pipelineName = run?.pipeline_name ?? pipeline?.name ?? "Untitled";
  const variables = pipeline?.variables ?? {};
  const variableEntries = Object.entries(variables);
  const managerSession = run ? `maestro-mgr-${run.run_id}` : null;

  const starredEntry = libraryPipelines.find((lp) => lp.name === pipelineName);
  const isStarred = !!starredEntry;

  return (
    <aside
      className="flex h-full flex-col bg-bg-2 overflow-y-auto"
      data-testid="pipeline-info-panel"
    >
      <div
        className="flex h-[36px] items-center justify-between border-b border-line px-3 font-medium text-fg-2"
        style={{ fontSize: "11.5px" }}
      >
        <span>Pipeline info</span>
        <button
          onClick={onClose}
          className="grid h-5 w-5 cursor-pointer place-items-center rounded text-fg-3 transition-colors hover:bg-bg-3 hover:text-fg"
          data-testid="info-panel-close"
        >
          <X size={12} />
        </button>
      </div>

      <div className="border-b border-line px-3 py-3" style={{ fontSize: "11.5px" }}>
        <div className="flex items-center gap-2">
          <span
            className={`h-2 w-2 shrink-0 rounded-full ${
              STATUS_DOT[run?.status ?? ""] ?? "bg-st-pending"
            }`}
          />
          <div className="min-w-0 flex-1">
            <div className="font-medium text-fg" data-testid="info-panel-name">
              {pipelineName}
            </div>
            <div
              className="mt-0.5 font-mono text-fg-4"
              style={{ fontSize: "10px" }}
            >
              {run ? `run ${run.run_id.slice(-8)} · ${pipeline?.version ?? "v1"}` : `template · ${pipeline?.version ?? "v1"}`}
            </div>
          </div>
          <InfoPanelStarButton
            isStarred={isStarred}
            starredId={starredEntry?.id ?? null}
            pipelineName={pipelineName}
            pipeline={pipeline}
            onLibraryChanged={onLibraryChanged}
          />
        </div>

        {variableEntries.length > 0 && (
          <div className="mt-3 flex flex-col gap-1" data-testid="info-panel-variables">
            {variableEntries.map(([name, def]) => (
              <div
                key={name}
                className="flex items-center justify-between rounded bg-bg-3 px-2 py-1"
                style={{ fontSize: "10.5px" }}
              >
                <span className="font-mono text-fg-3">{name}</span>
                <span className="font-mono text-fg-4">
                  {formatVariableValue(def.default)}
                </span>
              </div>
            ))}
          </div>
        )}
      </div>

      {managerSession ? (
        <div
          className="flex min-h-0 flex-1 flex-col"
          style={{ fontSize: "11.5px" }}
        >
          <div className="flex items-center gap-2 border-b border-line px-3 py-2">
            <Terminal size={14} className="text-fg-3" />
            <span className="text-fg-2" style={{ fontSize: "11px" }}>
              Pipeline Manager
            </span>
            <span
              className="font-mono text-fg-4"
              style={{ fontSize: "10px" }}
            >
              {managerSession}
            </span>
          </div>
          <TmuxTerminal
            session={managerSession}
            expanded
            status={run.status}
          />
        </div>
      ) : (
        <div className="px-3 py-3" style={{ fontSize: "11.5px" }}>
          <SectionHead title="Description" />
          <div
            className="mt-2 text-fg-3"
            style={{ fontSize: "12px", lineHeight: "1.55" }}
          >
            {pipeline?.name
              ? `Pipeline: ${pipeline.name}`
              : "No pipeline selected."}
          </div>
          <div
            className="mt-3 flex items-center gap-2 rounded border border-dashed border-line-soft bg-bg-3 px-3 py-2.5 text-fg-4"
            style={{ fontSize: "11.5px" }}
          >
            <Info size={14} className="shrink-0" />
            <span>
              No active run. Manager terminal appears here while a Run is in
              progress.
            </span>
          </div>
        </div>
      )}
    </aside>
  );
}

function InfoPanelStarButton({
  isStarred,
  starredId,
  pipelineName,
  pipeline,
  onLibraryChanged,
}: {
  isStarred: boolean;
  starredId: string | null;
  pipelineName: string;
  pipeline: PipelineDef | null;
  onLibraryChanged: () => void;
}) {
  const [busy, setBusy] = useState(false);

  async function handleToggle() {
    if (busy) return;
    setBusy(true);
    try {
      if (isStarred && starredId) {
        await deleteLibraryPipeline(starredId);
      } else if (pipeline) {
        const yaml = serializePipeline(pipeline);
        await saveLibraryPipeline(pipelineName, yaml);
      }
      onLibraryChanged();
    } catch {
      // ignore — endpoint may not be available yet (#58)
    } finally {
      setBusy(false);
    }
  }

  const tooltip = isStarred ? "Remove from library" : "Star as template";

  return (
    <Tooltip content={tooltip}>
      <button
        onClick={handleToggle}
        disabled={busy}
        className="grid h-6 w-6 shrink-0 cursor-pointer place-items-center rounded transition-colors hover:bg-bg-3 disabled:opacity-50"
        data-testid="info-panel-star"
      >
        <Star
          size={14}
          className={
            isStarred ? "fill-acc text-acc" : "fill-none text-fg-4"
          }
        />
      </button>
    </Tooltip>
  );
}

function formatVariableValue(value: unknown): string {
  if (Array.isArray(value)) return `[${value.join(", ")}]`;
  if (typeof value === "string") return `"${value}"`;
  return String(value ?? "");
}
