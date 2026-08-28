import { useState, useMemo, useRef, useEffect } from "react";
import { Info, Terminal, X, FileText, Code, Box, Loader, Bot, Copy, Download, ChevronDown, ChevronRight } from "lucide-react";
import { SectionHead } from "./InspectorPrimitives";
import TmuxTerminal from "./TmuxTerminal";
import DiffSection from "./DiffSection";
import type { LibraryPipelineEntry } from "../api";
import { fetchPipelineDocument, fetchRunPipelineDocument, openLibraryAssistant } from "../api";
import type { RunState, PipelineDef } from "../types";
import { isLiveRun } from "../types";
import { formatDuration, useRunDuration } from "../lib/runDuration";
import { formatEstCost } from "../lib/costLabel";
import { serializePipeline } from "../lib/serializePipeline";
import { highlightYaml } from "./yamlHighlight";

export type TabId = "info" | "manager" | "yaml" | "assistant";

function StatRow({
  label,
  children,
  testid,
}: {
  label: string;
  children: React.ReactNode;
  testid?: string;
}) {
  return (
    <div
      className="flex items-center justify-between rounded bg-bg-3 px-2 py-1"
      style={{ fontSize: "10.5px" }}
      data-testid={testid}
    >
      <span className="text-fg-3">{label}</span>
      <span className="font-mono text-fg-4">{children}</span>
    </div>
  );
}

interface Props {
  run: RunState | null;
  pipeline: PipelineDef | null;
  /** @deprecated Instance pipelines no longer have a library scope. */
  libraryPipelines?: LibraryPipelineEntry[];
  /** @deprecated Instance pipelines refresh through the edit store. */
  onLibraryChanged?: () => void;
  onClose: () => void;
  initialTab?: TabId;
  scrollToLine?: number;
  /** Library pipeline id of the active edit tab (#302 / ADR-0048). Present only
   *  for a library template tab (not a live Run); `null`/absent hides the
   *  Assistant tab. `PipelineDef` has no id, so it is threaded from the edit tab.
   *
   *  Since #594 it is a **visibility predicate only** — the panel no longer tells
   *  the assistant which template to work on (the daemon's focus does), and no
   *  longer owns its lifecycle (`useLibassistLifecycle`, mounted in `App`). */
  assistantId?: string | null;
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
  onClose,
  initialTab,
  scrollToLine,
  assistantId,
}: Props) {
  const pipelineName = run?.pipeline_name ?? pipeline?.name ?? "Untitled";
  const variables = pipeline?.variables ?? {};
  const variableEntries = Object.entries(variables);
  const managerSession = run ? `pdo-mgr-${run.run_id}` : null;

  const hasManager = !!managerSession;
  // #302: the Assistant is the mirror of the Manager — it exists only for a
  // library *template* (no live Run) with a resolvable pipeline id. Manager and
  // Assistant are therefore never both shown.
  const hasAssistant = !run && !!assistantId;
  const [activeTab, setActiveTab] = useState<TabId>(initialTab ?? "info");
  const resolvedTab =
    (activeTab === "manager" && !hasManager) ||
    (activeTab === "assistant" && !hasAssistant)
      ? "info"
      : activeTab;

  const tabs: { id: TabId; label: string; icon: typeof Info; show: boolean }[] = [
    { id: "info", label: "Info", icon: FileText, show: true },
    { id: "manager", label: "Manager", icon: Terminal, show: hasManager },
    { id: "assistant", label: "Assistant", icon: Bot, show: hasAssistant },
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
          // #397: no Tooltip here to borrow a name from — the `X` icon is
          // `aria-hidden`, so the label has to be explicit.
          aria-label="Close pipeline info"
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
          hasAssistant={hasAssistant}
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

      {resolvedTab === "assistant" && hasAssistant && assistantId && (
        // #594: NOT keyed on the pipeline. One shared assistant means switching
        // template must keep the same session and the same conversation — a
        // remount per pipeline is the exact opposite of sharing.
        <AssistantTab />
      )}

      {resolvedTab === "yaml" && (
        <YamlTab
          pipeline={pipeline}
          pipelineId={assistantId ?? null}
          runId={run?.run_id ?? null}
          scrollToLine={scrollToLine}
        />
      )}
    </aside>
  );
}

/**
 * The Assistant tab body (#302 / ADR-0048, #594 / ADR-0051): an inline `claude`
 * REPL that authors pipeline templates.
 *
 * **It creates, and never reaps.** Mounting still spawns the shared session on
 * demand, but the unmount cleanup that used to `DELETE` is gone: this component
 * unmounts whenever the panel closes, and the panel closes by itself on every
 * edit-tab switch (#385) — so reaping here killed the conversation each time the
 * user glanced at another template. The reap now lives at App level, keyed on
 * "no edit view left at all" (`useLibassistLifecycle`), with the daemon's idle
 * sweep behind it.
 *
 * It takes no props for the same reason: which template is being edited is the
 * daemon's focus, not this component's business.
 */
function AssistantTab() {
  const [session, setSession] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    openLibraryAssistant()
      .then((r) => {
        if (!cancelled) setSession(r.session);
      })
      .catch((e) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      style={{ fontSize: "11.5px" }}
      data-testid="assistant-tab"
    >
      <div className="flex items-center gap-2 border-b border-line px-3 py-2">
        <Bot size={14} className="text-fg-3" />
        <span className="text-fg-2" style={{ fontSize: "11px" }}>
          Pipeline Assistant
        </span>
        {session && (
          <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
            {session}
          </span>
        )}
      </div>
      {error ? (
        <div
          className="flex flex-1 items-center justify-center px-4 text-center text-st-failed"
          style={{ fontSize: "11.5px" }}
          data-testid="assistant-error"
        >
          Failed to start the assistant: {error}
        </div>
      ) : session ? (
        <TmuxTerminal session={session} expanded status="running" />
      ) : (
        <div
          className="flex flex-1 items-center justify-center gap-2 text-fg-4"
          style={{ fontSize: "11.5px" }}
          data-testid="assistant-loading"
        >
          <Loader size={14} className="animate-spin" />
          Starting the assistant…
        </div>
      )}
    </div>
  );
}

function InfoTab({
  run,
  pipeline,
  pipelineName,
  variables,
  hasAssistant,
}: {
  run: RunState | null;
  pipeline: PipelineDef | null;
  pipelineName: string;
  variables: [string, { default: unknown }][];
  hasAssistant: boolean;
}) {
  const durationMs = useRunDuration(run?.started_at, run?.completed_at, run?.status);
  const durationLabel = formatDuration(durationMs);
  const durationTicking = run != null && run.completed_at == null && isLiveRun(run.status);

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
          {/* Sandbox badge (#410): shown for any sandboxed Run (full/minimal). An
              `off`/host Run renders nothing — the field is absent on those. */}
          {run?.sandbox && run.sandbox !== "off" && (
            <span
              className="flex shrink-0 items-center gap-1 rounded bg-bg-3 px-1.5 py-0.5 font-mono text-fg-3"
              style={{ fontSize: "9.5px" }}
              data-testid="sandbox-badge"
              title={`This run is isolated in a Docker sandbox (${run.sandbox})`}
            >
              <Box size={10} className="shrink-0" />
              sandbox: {run.sandbox}
            </span>
          )}
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

      {/* Sandbox preparation banner (#410, amber): the image is being pulled/built
          at first use. Only while `sandbox_prep === "pending"` — it clears to
          `ready` once the container is about to run, so the Run never looks stuck. */}
      {run?.sandbox_prep === "pending" && (
        <div
          className="flex items-center gap-2 border-b border-st-await/30 bg-st-await-bg px-3 py-2"
          data-testid="sandbox-prep-banner"
        >
          <Loader size={14} className="shrink-0 animate-spin text-st-await" />
          <span
            className="text-st-await"
            style={{ fontSize: "11.5px", fontWeight: 500 }}
          >
            Preparing the sandbox — pulling/building the image…
          </span>
        </div>
      )}

      {run && (
        <div
          className="border-b border-line px-3 py-3"
          style={{ fontSize: "11.5px" }}
          data-testid="run-stats"
        >
          <SectionHead title="Stats" />
          <div className="mt-2 flex flex-col gap-1">
            <StatRow label="Duration" testid="stat-duration">
              <span className="flex items-center gap-1.5">
                {durationLabel ?? "—"}
                {durationTicking && (
                  <span
                    className="h-1.5 w-1.5 rounded-full bg-st-running animate-pulse"
                    title="live"
                    data-testid="stat-duration-live"
                  />
                )}
              </span>
            </StatRow>
            <StatRow label="Node sessions started" testid="stat-sessions">
              {(run.sessions_spawned ?? 0).toLocaleString()}
            </StatRow>
            <StatRow label="Lines changed" testid="stat-loc">
              {run.loc ? (
                <span className="flex items-center gap-1.5">
                  <span className="text-st-done">
                    +{run.loc.insertions.toLocaleString()}
                  </span>
                  <span className="text-st-failed">
                    −{run.loc.deletions.toLocaleString()}
                  </span>
                  <span className="text-fg-4">
                    {run.loc.files_changed.toLocaleString()}{" "}
                    {run.loc.files_changed === 1 ? "file" : "files"}
                  </span>
                </span>
              ) : (
                "—"
              )}
            </StatRow>
            <StatRow label="Est. cost" testid="stat-cost">
              {run.cost ? (
                (() => {
                  // Shared honesty helper (#272/#377): same vocabulary as the
                  // aggregated Stats charts.
                  const c = formatEstCost(
                    run.cost.usd,
                    run.cost.partial,
                    run.cost.unpriced_models,
                    // #553: "—" + reason when a node ran on a harness with no cost
                    // source (e.g. opencode) — never a misleading $0.
                    run.cost.uncosted_harnesses ?? [],
                    // #615: ventilate a mixed Run's total by harness.
                    run.cost.by_harness ?? [],
                  );
                  // Show the per-harness breakdown only when it says more than the
                  // total already does: a genuinely mixed Run (≥2 slices), or — when
                  // the total is withheld as "—" — any slice at all, since then even
                  // one says something the total cannot (#617 FP).
                  const slices = c.ventilation ?? [];
                  const ventilated = slices.length > (c.text === "—" ? 0 : 1);
                  return (
                    <span className="flex flex-col items-end gap-0.5" title={c.title}>
                      <span className="flex items-center gap-1">
                        {c.text}
                        {c.dagger && <span className="text-st-await">†</span>}
                      </span>
                      {ventilated && (
                        <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
                          {slices
                            .map((v) => `${v.text} via ${v.harness}`)
                            .join(" · ")}
                        </span>
                      )}
                    </span>
                  );
                })()
              ) : (
                "—"
              )}
            </StatRow>
          </div>
        </div>
      )}

      <DiffSection run={run} />

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
              {hasAssistant
                ? "This is a template. Use the Assistant tab to author it in natural language; the Manager tab becomes available while a Run is in progress."
                : "No active run. The Manager tab becomes available while a Run is in progress."}
            </span>
          </div>
        )}
      </div>
    </>
  );
}

function YamlTab({
  pipeline,
  pipelineId,
  runId,
  scrollToLine,
}: {
  pipeline: PipelineDef | null;
  pipelineId: string | null;
  runId: string | null;
  scrollToLine?: number;
}) {
  const preRef = useRef<HTMLPreElement>(null);
  const fallbackYaml = useMemo(
    () => (pipeline ? serializePipeline(pipeline) : ""),
    [pipeline],
  );
  const [yaml, setYaml] = useState(fallbackYaml);
  const [copied, setCopied] = useState(false);
  const [copyError, setCopyError] = useState(false);
  const [showExcluded, setShowExcluded] = useState(false);

  useEffect(() => {
    let cancelled = false;
    const request = runId
      ? fetchRunPipelineDocument(runId)
      : pipelineId
        ? fetchPipelineDocument(pipelineId)
        : Promise.resolve(fallbackYaml);
    request
      .then((document) => {
        if (!cancelled) setYaml(document);
      })
      .catch(() => {
        if (!cancelled) setYaml(fallbackYaml);
      });
    return () => {
      cancelled = true;
    };
  }, [fallbackYaml, pipelineId, runId]);

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

  async function copyDocument() {
    setCopyError(false);
    try {
      await navigator.clipboard.writeText(yaml);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch {
      setCopyError(true);
    }
  }

  const pipelineName = pipeline.name || "pipeline";

  function downloadDocument() {
    const url = URL.createObjectURL(new Blob([yaml], { type: "application/yaml" }));
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = `${pipelineName}.pdo.yaml`;
    anchor.click();
    URL.revokeObjectURL(url);
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-auto">
      <div className="border-b border-line bg-bg-3 px-3 py-2" data-testid="portable-document-bar">
        <div className="flex items-center gap-2">
          <span className="h-1.5 w-1.5 rounded-full bg-acc" />
          <span className="font-medium text-fg-2" style={{ fontSize: "11px" }}>
            Portable document · v1
          </span>
          <button
            className="ml-auto flex items-center gap-1 rounded px-1.5 py-1 text-fg-3 hover:bg-bg-4 hover:text-fg"
            onClick={copyDocument}
          >
            <Copy size={11} />
            {copied ? "✓ Copied" : "Copy"}
          </button>
          <button
            className="flex items-center gap-1 rounded px-1.5 py-1 text-fg-3 hover:bg-bg-4 hover:text-fg"
            onClick={downloadDocument}
          >
            <Download size={11} />
            Download
          </button>
        </div>
        {copyError && (
          <div className="mt-1 text-st-failed" role="alert">
            Clipboard access was denied. Use Download instead.
          </div>
        )}
        {runId && (
          <p className="mt-1 text-fg-4" style={{ fontSize: "10px" }}>
            This is the pipeline that ran, not the Run. Runtime values are not included.
          </p>
        )}
        <button
          className="mt-1.5 flex items-center gap-1 text-fg-4 hover:text-fg-2"
          style={{ fontSize: "10.5px" }}
          onClick={() => setShowExcluded((value) => !value)}
        >
          {showExcluded ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
          Not included
        </button>
        {showExcluded && (
          <p className="mt-1 text-fg-4" style={{ fontSize: "10px" }}>
            Secrets, environment, runtime values, and instance configuration. Named agent
            profiles become Inherit; shared nodes become ordinary nodes.
          </p>
        )}
      </div>
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

function formatVariableValue(value: unknown): string {
  if (Array.isArray(value)) return `[${value.join(", ")}]`;
  if (typeof value === "string") return `"${value}"`;
  return String(value ?? "");
}
