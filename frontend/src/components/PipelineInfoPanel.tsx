import { useState, useMemo, useRef, useEffect } from "react";
import { Star, Info, Terminal, X, FileText, Code } from "lucide-react";
import { Tooltip } from "./ui/tooltip";
import { SectionHead } from "./InspectorPrimitives";
import TmuxTerminal from "./TmuxTerminal";
import { saveLibraryPipeline, deleteLibraryPipeline } from "../api";
import type { LibraryPipelineEntry } from "../api";
import type { RunState, PipelineDef } from "../types";
import { serializePipeline } from "../stores/editStore";

export type TabId = "info" | "manager" | "yaml";

interface Props {
  run: RunState | null;
  pipeline: PipelineDef | null;
  libraryPipelines: LibraryPipelineEntry[];
  onLibraryChanged: () => void;
  onClose: () => void;
  initialTab?: TabId;
  scrollToLine?: number;
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
  initialTab,
  scrollToLine,
}: Props) {
  const pipelineName = run?.pipeline_name ?? pipeline?.name ?? "Untitled";
  const variables = pipeline?.variables ?? {};
  const variableEntries = Object.entries(variables);
  const managerSession = run ? `maestro-mgr-${run.run_id}` : null;

  const starredEntry = libraryPipelines.find((lp) => lp.name === pipelineName);
  const isStarred = !!starredEntry;

  const hasManager = !!managerSession;
  const [activeTab, setActiveTab] = useState<TabId>(initialTab ?? "info");
  const resolvedTab = activeTab === "manager" && !hasManager ? "info" : activeTab;

  const tabs: { id: TabId; label: string; icon: typeof Info; show: boolean }[] = [
    { id: "info", label: "Info", icon: FileText, show: true },
    { id: "manager", label: "Manager", icon: Terminal, show: hasManager },
    { id: "yaml", label: "YAML", icon: Code, show: true },
  ];

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

      <div
        className="flex border-b border-line"
        style={{ fontSize: "11px" }}
      >
        {tabs
          .filter((t) => t.show)
          .map((t) => (
            <button
              key={t.id}
              data-testid={`info-tab-${t.id}`}
              onClick={() => setActiveTab(t.id)}
              className={`flex items-center gap-1.5 px-3 py-1.5 transition-colors cursor-pointer ${
                resolvedTab === t.id
                  ? "border-b-2 border-acc text-fg font-medium"
                  : "text-fg-3 hover:text-fg-2"
              }`}
            >
              <t.icon size={12} />
              {t.label}
            </button>
          ))}
      </div>

      {resolvedTab === "info" && (
        <InfoTab
          run={run}
          pipeline={pipeline}
          pipelineName={pipelineName}
          variables={variableEntries}
          isStarred={isStarred}
          starredId={starredEntry?.id ?? null}
          onLibraryChanged={onLibraryChanged}
        />
      )}

      {resolvedTab === "manager" && managerSession && run && (
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
      )}

      {resolvedTab === "yaml" && (
        <YamlTab pipeline={pipeline} scrollToLine={scrollToLine} />
      )}
    </aside>
  );
}

function InfoTab({
  run,
  pipeline,
  pipelineName,
  variables,
  isStarred,
  starredId,
  onLibraryChanged,
}: {
  run: RunState | null;
  pipeline: PipelineDef | null;
  pipelineName: string;
  variables: [string, { default: unknown }][];
  isStarred: boolean;
  starredId: string | null;
  onLibraryChanged: () => void;
}) {
  return (
    <>
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
            starredId={starredId}
            pipelineName={pipelineName}
            pipeline={pipeline}
            onLibraryChanged={onLibraryChanged}
          />
        </div>

        {variables.length > 0 && (
          <div className="mt-3 flex flex-col gap-1" data-testid="info-panel-variables">
            {variables.map(([name, def]) => (
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
        {!run && (
          <div
            className="mt-3 flex items-center gap-2 rounded border border-dashed border-line-soft bg-bg-3 px-3 py-2.5 text-fg-4"
            style={{ fontSize: "11.5px" }}
          >
            <Info size={14} className="shrink-0" />
            <span>
              No active run. The Manager tab becomes available while a Run is in
              progress.
            </span>
          </div>
        )}
      </div>
    </>
  );
}

function YamlTab({ pipeline, scrollToLine }: { pipeline: PipelineDef | null; scrollToLine?: number }) {
  const preRef = useRef<HTMLPreElement>(null);
  const yaml = useMemo(
    () => (pipeline ? serializePipeline(pipeline) : ""),
    [pipeline],
  );

  useEffect(() => {
    if (scrollToLine == null || !preRef.current) return;
    const lineHeight = 11 * 1.6;
    const scrollTop = Math.max(0, (scrollToLine - 3) * lineHeight);
    preRef.current.scrollTop = scrollTop;
  }, [scrollToLine]);

  if (!pipeline) {
    return (
      <div className="flex flex-1 items-center justify-center text-fg-4" style={{ fontSize: "12px" }}>
        No pipeline loaded.
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-auto">
      <pre
        ref={preRef}
        className="flex-1 overflow-auto p-3 font-mono text-fg-3 select-text"
        style={{ fontSize: "11px", lineHeight: "1.6", tabSize: 2 }}
        data-testid="info-yaml-content"
      >
        {highlightYaml(yaml, scrollToLine)}
      </pre>
    </div>
  );
}

function highlightYaml(yaml: string, errorLine?: number): React.ReactNode {
  const lines = yaml.split("\n");
  return lines.map((line, i) => {
    const lineNum = i + 1;
    const isError = errorLine != null && lineNum === errorLine;
    const commentIdx = line.indexOf("#");
    return (
      <span
        key={i}
        className={isError ? "bg-st-failed/20" : undefined}
        data-line={lineNum}
      >
        {commentIdx >= 0 ? (
          <>
            {highlightLine(line.slice(0, commentIdx))}
            <span className="text-fg-4 italic">{line.slice(commentIdx)}</span>
          </>
        ) : (
          highlightLine(line)
        )}
        {i < lines.length - 1 ? "\n" : null}
      </span>
    );
  });
}

function highlightLine(line: string): React.ReactNode {
  const keyMatch = line.match(/^(\s*-?\s*)([a-zA-Z_][\w]*)\s*:/);
  if (keyMatch) {
    const [, indent, key] = keyMatch;
    const rest = line.slice(keyMatch[0].length);
    return (
      <>
        {indent}
        <span className="text-acc">{key}</span>
        <span className="text-fg-4">:</span>
        {highlightValue(rest)}
      </>
    );
  }

  const listMatch = line.match(/^(\s*-\s+)(.*)/);
  if (listMatch) {
    const [, prefix, rest] = listMatch;
    return (
      <>
        <span className="text-fg-4">{prefix}</span>
        {highlightValue(rest)}
      </>
    );
  }

  return line;
}

function highlightValue(value: string): React.ReactNode {
  const trimmed = value.trimStart();
  const leading = value.slice(0, value.length - trimmed.length);

  if (/^".*"$/.test(trimmed) || /^'.*'$/.test(trimmed)) {
    return <>{leading}<span className="text-st-await">{trimmed}</span></>;
  }

  if (/^(true|false|null)$/.test(trimmed)) {
    return <>{leading}<span className="text-st-running">{trimmed}</span></>;
  }

  if (/^-?\d+(\.\d+)?$/.test(trimmed)) {
    return <>{leading}<span className="text-st-done">{trimmed}</span></>;
  }

  if (/^\{.*\}$/.test(trimmed)) {
    return <>{leading}<span className="text-fg-3">{trimmed}</span></>;
  }

  return <>{leading}<span className="text-fg-2">{trimmed}</span></>;
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
