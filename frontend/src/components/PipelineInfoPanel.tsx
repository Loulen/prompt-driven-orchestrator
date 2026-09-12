import { useState, useMemo, useRef, useEffect, useCallback } from "react";
import { Info, Terminal, X, FileText, Code, Box, Loader, Bot, Copy, Download, ChevronDown, ChevronRight, Play, PowerOff, FileDiff, FolderGit2 } from "lucide-react";
import { SectionHead } from "./InspectorPrimitives";
import TmuxTerminal from "./TmuxTerminal";
import DiffTab from "./DiffTab";
import RepositoriesSection from "./RepositoriesSection";
import { deliverySignature } from "../lib/diffTab";
import { useReviewUnread } from "../hooks/useReviewUnread";
import type { CollapsedFiles } from "../lib/diffTab";
import type { LibraryPipelineEntry } from "../api";
import { fetchPipelineDocument, fetchPipelineSkillsSidecar, fetchRunPipelineDocument, fetchRunPipelineSkillsSidecar, openLibraryAssistant, startRunManager, stopRunManager } from "../api";
import type { RunState, PipelineDef } from "../types";
import { isLiveRun } from "../types";
import { formatDuration, useRunDuration } from "../lib/runDuration";
import { formatEstCost } from "../lib/costLabel";
import { serializePipeline } from "../lib/serializePipeline";
import { highlightYaml } from "./yamlHighlight";

export type TabId = "info" | "diff" | "repositories" | "manager" | "yaml" | "assistant";

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
  /** Manager on demand: refetch the Run state after a start/stop, so
   *  `has_manager` flips and the tab swaps its empty state for the terminal
   *  (the WS push usually lands first; this is the belt to its braces). */
  onRefreshRun?: () => void;
  /** Manager on demand: the empty state's « enable it for every run in
   *  Settings » link — the host opens the surface on Agents › Pipeline
   *  Manager (the existing programmatic entry, #690 story 18). */
  onOpenSettings?: () => void;
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
  onRefreshRun,
  onOpenSettings,
}: Props) {
  const pipelineName = run?.pipeline_name ?? pipeline?.name ?? "Untitled";
  const variables = pipeline?.variables ?? {};
  const variableEntries = Object.entries(variables);
  const managerSession = run ? `pdo-mgr-${run.run_id}` : null;

  // Manager on demand: the Manager tab shows for EVERY live Run — an empty
  // state with a Start button when no session exists (the new default
  // posture), the terminal when one does. It also stays for a
  // completed/archived Run whose session survives until cleanup (post-mortem
  // interrogation). Only a template (no Run at all) hides it — the Assistant
  // tab covers templates instead (#302).
  // #302: the Assistant is the mirror of the Manager — it exists only for a
  // library *template* (no live Run) with a resolvable pipeline id. Manager and
  // Assistant are therefore never both shown.
  const hasAssistant = !run && !!assistantId;
  const [activeTab, setActiveTab] = useState<TabId>(initialTab ?? "info");
  const resolvedTab =
    ((activeTab === "manager" || activeTab === "diff" || activeTab === "repositories") && !run) ||
    (activeTab === "assistant" && !hasAssistant)
      ? "info"
      : activeTab;

  // #748: the Diff tab's expand/collapse state, keyed by file path, lives here so
  // it survives `Info ↔ Diff` and a "Diff changed · Reload". Reset per Run.
  // Keyed by Run id, so a Run switch under a mounted panel starts fresh.
  const [diffCollapsedByRun, setDiffCollapsedByRun] = useState<Record<string, Set<string>>>({});
  const runId = run?.run_id ?? null;
  const diffCollapsed: CollapsedFiles = runId ? (diffCollapsedByRun[runId] ?? null) : null;
  const onDiffCollapsedChange = useCallback(
    (next: Set<string>) => {
      if (!runId) return;
      setDiffCollapsedByRun((prev) => ({ ...prev, [runId]: next }));
    },
    [runId],
  );

  // The blue dot on the Diff tab: the tip moved (a node delivered) since the
  // tab was last looked at. Recorded on entering and on leaving the tab.
  const deliverySig = run ? deliverySignature(run) : "";
  const [seenDeliverySig, setSeenDeliverySig] = useState(deliverySig);
  const selectTab = (id: TabId) => {
    if (id === "diff" || resolvedTab === "diff") setSeenDeliverySig(deliverySig);
    setActiveTab(id);
  };
  const nudgeDiff =
    run != null && resolvedTab !== "diff" && seenDeliverySig !== deliverySig && deliverySig !== "";
  // #751: sent review comments with an unread reply — a solid count pill that
  // replaces the blue dot while > 0 (a number beats a dot; both mean "come look").
  // Cleared by opening the Review page (per browser, localStorage).
  const unreadReplies = useReviewUnread(run);

  const tabs: { id: TabId; label: string; icon: typeof Info; show: boolean }[] = [
    { id: "info", label: "Info", icon: FileText, show: true },
    { id: "diff", label: "Diff", icon: FileDiff, show: run != null },
    // #752 (closing the second half of #566): the Repositories view is a tab of
    // the Run panel — `Info | Diff | Repositories | Manager | YAML` — reachable
    // on any selection, archived Runs included (frozen list). No dot: nothing
    // asynchronous happens to repositories; errors show inline in the tab.
    { id: "repositories", label: "Repositories", icon: FolderGit2, show: run != null },
    { id: "manager", label: "Manager", icon: Terminal, show: run != null },
    { id: "assistant", label: "Assistant", icon: Bot, show: hasAssistant },
    { id: "yaml", label: "YAML", icon: Code, show: true },
  ];

  // The quiet nudge (manager on demand): the exact moment a manager helps most
  // is when the Run is parked on the user — an amber dot on the tab points it
  // out without banners or auto-switching.
  const nudgeManager = run?.status === "awaiting_user" && !(run.has_manager ?? false);

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
              onClick={() => selectTab(t.id)}
              className={`flex items-center gap-1.5 px-2 py-1.5 transition-colors cursor-pointer ${
                resolvedTab === t.id
                  ? "border-b-2 border-acc text-fg font-medium"
                  : "text-fg-3 hover:text-fg-2"
              }`}
            >
              <t.icon size={12} />
              {t.label}
              {t.id === "manager" && nudgeManager && (
                <span
                  className="h-1.5 w-1.5 rounded-full bg-st-await"
                  aria-hidden
                  data-testid="manager-tab-dot"
                />
              )}
              {t.id === "diff" && unreadReplies > 0 ? (
                <span
                  className="inline-flex h-[14px] min-w-[14px] items-center justify-center rounded-[7px] bg-st-running px-1 font-semibold text-white"
                  style={{ fontSize: "9.5px" }}
                  title={`${unreadReplies} review comment${unreadReplies === 1 ? "" : "s"} with an unread reply — open the Review page`}
                  data-testid="diff-tab-unread"
                >
                  {unreadReplies}
                </span>
              ) : (
                t.id === "diff" &&
                nudgeDiff && (
                  <span
                    className="h-1.5 w-1.5 rounded-full bg-st-running"
                    aria-hidden
                    data-testid="diff-tab-dot"
                  />
                )
              )}
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
          onOpenDiff={() => selectTab("diff")}
        />
      )}

      {resolvedTab === "diff" && run && (
        <DiffTab
          key={run.run_id}
          run={run}
          collapsed={diffCollapsed}
          onCollapsedChange={onDiffCollapsedChange}
        />
      )}

      {resolvedTab === "repositories" && run && (
        <div data-testid="repositories-tab" className="flex flex-col">
          {run.target_repo ? (
            <RepositoriesSection key={run.run_id} run={run} onEdited={onRefreshRun} />
          ) : (
            <div className="px-3 py-3 text-fg-4" style={{ fontSize: "11px" }} data-testid="repositories-tab-empty">
              This Run recorded no target repository — it works in the daemon's own checkout.
            </div>
          )}
        </div>
      )}

      {resolvedTab === "manager" && run && managerSession && (
        <ManagerTab
          key={run.run_id}
          run={run}
          session={managerSession}
          onRefreshRun={onRefreshRun}
          onOpenSettings={onOpenSettings}
        />
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
 * The Manager tab body (manager on demand): the Run's Pipeline Manager, started
 * only when the user asks for it. Four states, keyed on the OBSERVED
 * `run.has_manager` fact the daemon probes in tmux at fetch time — never on a
 * constructed session name (the pre-change code would happily mount a terminal
 * for a session that did not exist):
 *
 * - **empty** — the new default posture: a centered card explaining what the
 *   manager would do and what it costs, a Start button, and a link to flip the
 *   per-Run default in Settings;
 * - **starting** — the POST round-trip plus the gap until the next run-state
 *   fetch flips `has_manager` (the same beat the Assistant tab has);
 * - **error** — a failed spawn, with a Retry, mirroring `AssistantTab`;
 * - **live** — today's terminal, unchanged, plus a Stop control with a
 *   confirmation: cost control is the point of the feature, so the session
 *   must be killable from the panel, not only by the orphan sweep.
 *
 * The tab (and its Run) can be left and revisited freely: nothing here stops
 * the session — the manager outlives Run completion for post-mortem questions.
 */
function ManagerTab({
  run,
  session,
  onRefreshRun,
  onOpenSettings,
}: {
  run: RunState;
  session: string;
  onRefreshRun?: () => void;
  onOpenSettings?: () => void;
}) {
  const hasManager = run.has_manager ?? false;
  const [starting, setStarting] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [confirmingStop, setConfirmingStop] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const start = async () => {
    setError(null);
    setStarting(true);
    try {
      // Idempotent on the daemon (a double-click is a benign re-answer). The
      // "starting" state holds until the refreshed run state flips
      // `has_manager` — the terminal mounts then, not here.
      await startRunManager(run.run_id);
      onRefreshRun?.();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setStarting(false);
    }
  };

  const stop = async () => {
    setStopping(true);
    try {
      await stopRunManager(run.run_id);
      onRefreshRun?.();
      setConfirmingStop(false);
      // Done with the transient state: once the refreshed run state lands, the
      // empty state — not a stale "starting…" — must be what the tab shows.
      setStarting(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setStopping(false);
    }
  };

  if (hasManager) {
    return (
      <div
        className="flex min-h-0 flex-1 flex-col"
        style={{ fontSize: "11.5px" }}
        data-testid="manager-live"
      >
        <div className="flex items-center gap-2 border-b border-line px-3 py-2">
          <Terminal size={14} className="shrink-0 text-fg-3" />
          <div className="flex min-w-0 flex-1 flex-col">
            <span className="text-fg-2" style={{ fontSize: "11px" }}>
              Pipeline Manager
            </span>
            <span
              className="truncate font-mono text-fg-4"
              style={{ fontSize: "10px" }}
            >
              {session}
            </span>
          </div>
          {confirmingStop ? (
            <span className="flex shrink-0 items-center gap-1.5">
              <span className="text-fg-3" style={{ fontSize: "10.5px" }}>
                Stop the manager?
              </span>
              <button
                onClick={() => void stop()}
                disabled={stopping}
                className="rounded border border-st-failed/40 bg-st-failed-bg px-2 py-0.5 text-st-failed transition-colors hover:border-st-failed disabled:opacity-40"
                style={{ fontSize: "10.5px" }}
                data-testid="manager-stop-confirm"
              >
                {stopping ? "Stopping…" : "Stop"}
              </button>
              <button
                onClick={() => setConfirmingStop(false)}
                className="rounded border border-line-strong bg-bg-3 px-2 py-0.5 text-fg-3 transition-colors hover:text-fg-2"
                style={{ fontSize: "10.5px" }}
                data-testid="manager-stop-cancel"
              >
                Cancel
              </button>
            </span>
          ) : (
            <button
              onClick={() => setConfirmingStop(true)}
              className="flex shrink-0 items-center gap-1 rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg-3 transition-colors hover:border-st-failed hover:text-st-failed"
              style={{ fontSize: "10.5px" }}
              data-testid="manager-stop"
              title="Stop the manager session (survives until cleanup otherwise)"
            >
              <PowerOff size={11} />
              Stop
            </button>
          )}
        </div>
        {error && (
          <div
            className="border-b border-st-failed/30 bg-st-failed-bg px-3 py-1.5 text-st-failed"
            style={{ fontSize: "10.5px" }}
            role="alert"
            data-testid="manager-error"
          >
            {error}
          </div>
        )}
        <TmuxTerminal session={session} expanded status={run.status} />
      </div>
    );
  }

  if (starting) {
    return (
      <div
        className="flex flex-1 flex-col items-center justify-center gap-3 text-fg-4"
        style={{ fontSize: "11.5px" }}
        data-testid="manager-starting"
      >
        <Loader size={18} className="animate-spin text-acc" />
        Starting the manager…
      </div>
    );
  }

  if (error) {
    return (
      <div
        className="flex flex-1 flex-col items-center justify-center gap-3 px-6 text-center"
        style={{ fontSize: "11.5px" }}
        data-testid="manager-start-error"
      >
        <div className="text-st-failed" role="alert">
          Failed to start the manager: {error}
        </div>
        <button
          onClick={() => void start()}
          className="rounded-md bg-acc px-3 py-1.5 font-medium text-on-acc transition-colors hover:bg-acc-dim"
          style={{ fontSize: "11.5px" }}
          data-testid="manager-retry"
        >
          Retry
        </button>
      </div>
    );
  }

  return (
    <div
      className="flex flex-1 flex-col items-center justify-center gap-3 px-8 text-center"
      style={{ fontSize: "11.5px" }}
      data-testid="manager-empty-state"
    >
      <div className="grid h-11 w-11 place-items-center rounded-lg bg-bg-3 text-fg-3">
        <Terminal size={18} />
      </div>
      <div className="font-medium text-fg" style={{ fontSize: "13px" }}>
        No manager on this run
      </div>
      <p
        className="max-w-[260px] text-fg-3"
        style={{ fontSize: "11.5px", lineHeight: 1.55 }}
      >
        The manager is a conversational agent that can drive this run — retry
        nodes, resolve merges, unblock it when it waits on you. It is off by
        default to keep costs down.
      </p>
      <button
        onClick={() => void start()}
        className="mt-1 flex items-center gap-1.5 rounded-md bg-acc px-3.5 py-2 font-medium text-on-acc transition-colors hover:bg-acc-dim"
        style={{ fontSize: "12px" }}
        data-testid="manager-start"
      >
        <Play size={12} />
        Start manager
      </button>
      <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
        or{" "}
        <button
          onClick={() => onOpenSettings?.()}
          className="cursor-pointer text-fg-3 underline decoration-fg-5 underline-offset-2 transition-colors hover:text-fg"
          data-testid="manager-enable-settings"
        >
          enable it for every run in Settings
        </button>
      </div>
    </div>
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
  onOpenDiff,
}: {
  run: RunState | null;
  pipeline: PipelineDef | null;
  pipelineName: string;
  variables: [string, { default: unknown }][];
  hasAssistant: boolean;
  /** #748: the Changes stat is the link to what it counts — the Diff tab. */
  onOpenDiff: () => void;
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

        {/* #752: what the standalone Run-info sidebar carried besides Repositories
            lives here now — the failure / awaiting reason (#503, #598), the frozen
            harness (#551) and the editing note (#315). Clicking a red dot lands on
            this tab, so the failure must be the first thing it says. */}
        {run?.failure_reason && (
          <div
            className="mt-2 rounded border border-st-failed/30 bg-st-failed-bg px-2 py-1.5 text-fg-2"
            style={{ fontSize: "10.5px" }}
            data-testid="run-failure-reason"
          >
            <div className="font-medium text-st-failed">
              {run.status === "halted" ? "Halted" : run.status === "skipped" ? "Skipped" : "Failed"}
            </div>
            <div className="mt-0.5 break-words">{run.failure_reason}</div>
          </div>
        )}
        {run?.awaiting_reason && (
          <div
            className="mt-2 rounded border border-st-await/30 bg-st-await-bg px-2 py-1.5 text-fg-2"
            style={{ fontSize: "10.5px" }}
            data-testid="run-awaiting-reason"
          >
            {/* #588: only an incident carries a machine slug; a declared wait is
                the agent's question (or an awaiting child), not an interruption. */}
            <div className="font-medium text-st-await">
              {run.awaiting_reason_code ? "Interrupted · awaiting you" : "Awaiting you"}
            </div>
            <div className="mt-0.5 break-words">{run.awaiting_reason}</div>
          </div>
        )}
        {run?.harness && (
          <div className="mt-2 flex items-center gap-1.5 text-fg-3" style={{ fontSize: "10.5px" }} data-testid="run-harness">
            <span className="text-fg-4">Harness</span>
            <span className="rounded bg-bg-3 px-1.5 py-0.5 font-mono text-fg-2">{run.harness}</span>
          </div>
        )}
        {run && (
          <div
            className="mt-2 rounded border border-line-strong bg-bg-3 px-2 py-1.5 text-fg-3"
            style={{ fontSize: "10.5px" }}
            data-testid="run-info-note"
          >
            {run.status === "archived"
              ? "Archived run · read-only · outputs preserved"
              : "Editing run-scoped pipeline · changes sync to template"}
          </div>
        )}

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
            <StatRow label="Changes" testid="stat-loc">
              {run.loc ? (
                // #748: the LOC stat opens the Diff tab — the count IS what the
                // tab shows (same endpoint bounds), so the number links to it.
                <button
                  onClick={onOpenDiff}
                  className="flex items-center gap-1.5 rounded px-1 -mx-1 transition-colors hover:bg-bg-4 hover:text-fg-2 cursor-pointer"
                  data-testid="stat-loc-open-diff"
                  title="Open the Diff tab"
                >
                  <span className="text-st-done">
                    +{run.loc.insertions.toLocaleString()}
                  </span>
                  <span className="text-st-failed">
                    −{run.loc.deletions.toLocaleString()}
                  </span>
                  <span className="text-fg-4">
                    · {run.loc.files_changed.toLocaleString()}{" "}
                    {run.loc.files_changed === 1 ? "file" : "files"}
                  </span>
                  <span className="text-fg-4" aria-hidden>↗</span>
                </button>
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
  const [sidecarError, setSidecarError] = useState<string | null>(null);
  // #673 / ADR-0062: the skills the nodes select travel beside the YAML, in a
  // sidecar `<pipeline>.skills/<id>/…` — content, not instance configuration.
  const skillCount = useMemo(() => {
    const ids = new Set<string>();
    for (const node of pipeline?.nodes ?? []) {
      for (const skill of node.skills ?? []) ids.add(skill.id);
    }
    return ids.size;
  }, [pipeline]);

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

  function saveBlob(blob: Blob, filename: string) {
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = filename;
    anchor.click();
    URL.revokeObjectURL(url);
  }

  function downloadDocument() {
    saveBlob(new Blob([yaml], { type: "application/yaml" }), `${pipelineName}.pdo.yaml`);
  }

  async function downloadSkillsSidecar() {
    setSidecarError(null);
    try {
      const blob = runId
        ? await fetchRunPipelineSkillsSidecar(runId)
        : pipelineId
          ? await fetchPipelineSkillsSidecar(pipelineId)
          : null;
      if (!blob) {
        setSidecarError("No skill of this pipeline is in the bank: nothing to export.");
        return;
      }
      saveBlob(blob, `${pipelineName}.skills.zip`);
    } catch (e) {
      setSidecarError(e instanceof Error ? e.message : String(e));
    }
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
          {skillCount > 0 && (
            <button
              className="flex items-center gap-1 rounded px-1.5 py-1 text-fg-3 hover:bg-bg-4 hover:text-fg"
              onClick={() => void downloadSkillsSidecar()}
              title={`The ${skillCount} skill${skillCount === 1 ? "" : "s"} this pipeline selects, as ${pipelineName}.skills/<id>/… to unpack next to the YAML`}
              data-testid="download-skills-sidecar"
            >
              <Download size={11} />
              Skills ({skillCount})
            </button>
          )}
        </div>
        {copyError && (
          <div className="mt-1 text-st-failed" role="alert">
            Clipboard access was denied. Use Download instead.
          </div>
        )}
        {sidecarError && (
          <div className="mt-1 text-st-failed" role="alert" data-testid="skills-sidecar-error">
            {sidecarError}
          </div>
        )}
        {skillCount > 0 && (
          <p className="mt-1 text-fg-4" style={{ fontSize: "10px" }} data-testid="skills-sidecar-note">
            {skillCount} skill{skillCount === 1 ? "" : "s"} referenced by id. Their content travels in
            the sidecar <code>{pipelineName}.skills/</code>; import both so the bank recreates them.
          </p>
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
            profiles become Inherit; shared nodes become ordinary nodes. Skills are content,
            not configuration: they travel in the separate skills sidecar.
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
