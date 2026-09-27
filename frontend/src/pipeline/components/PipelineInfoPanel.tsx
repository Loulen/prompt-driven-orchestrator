/**
 * The info panel beside the canvas: the pipeline's or the Run's Info, Diff, Repositories, Manager, YAML and Assistant tabs.
 */

import { useState, useCallback } from "react";
import { Info, Terminal, X, FileText, Code, Bot, FileDiff, FolderGit2 } from "lucide-react";
import AssistantPanel from "../../session/components/AssistantPanel";
import DiffTab from "../../components/DiffTab";
import RepositoriesSection from "../../run/components/RepositoriesSection";
import { deliverySignature } from "../../lib/diffTab";
import { useReviewUnread } from "../../hooks/useReviewUnread";
import type { CollapsedFiles } from "../../lib/diffTab";
import type { LibraryPipelineEntry } from "../api/libraryPipelines";
import type { RunState } from "../../shared/generated/RunState";
import type { EditorPipelineDef } from "../model/pipeline";

import { InfoTab } from "./InfoTab";
import { ManagerTab } from "./ManagerTab";
import { YamlTab } from "./YamlTab";

export type TabId = "info" | "diff" | "repositories" | "manager" | "yaml" | "assistant";

interface Props {
  run: RunState | null;
  pipeline: EditorPipelineDef | null;
  /** @deprecated Instance pipelines no longer have a library scope. */
  libraryPipelines?: LibraryPipelineEntry[];
  /** @deprecated Instance pipelines refresh through the edit store. */
  onLibraryChanged?: () => void;
  onClose: () => void;
  initialTab?: TabId;
  scrollToLine?: number;
  /** Library pipeline id of the active edit tab (#302 / ADR-0048). Present only
   *  for a library template tab (not a live Run); `null`/absent hides the
   *  Assistant tab. `EditorPipelineDef` has no id, so it is threaded from the edit tab.
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
        <AssistantPanel />
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
 * - **error** — a failed spawn, with a Retry, mirroring `AssistantPanel`;
 * - **live** — today's terminal, unchanged, plus a Stop control with a
 *   confirmation: cost control is the point of the feature, so the session
 *   must be killable from the panel, not only by the orphan sweep.
 *
 * The tab (and its Run) can be left and revisited freely: nothing here stops
 * the session — the manager outlives Run completion for post-mortem questions.
 */
