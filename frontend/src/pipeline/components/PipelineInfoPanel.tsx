/**
 * The info panel beside the canvas: the pipeline's or the Run's Info, Versions, Diff, Repositories, Manager, YAML and Assistant tabs.
 */

import { useState, useCallback, useMemo } from "react";
import { Bot, FileCode, FolderGit2, GitCompare, History, Info, Puzzle, Sparkles, X } from "lucide-react";
import { EmptyState, IconButton } from "../../shared/ui/kit";
import InfoTabs, { type InfoTabItem } from "./InfoTabs";
import "./inspect.css";
import AssistantPanel from "../../session/components/AssistantPanel";
import DiffTab from "../../review/components/DiffTab";
import RepositoriesSection from "../../run/components/RepositoriesSection";
import { deliverySignature } from "../../review/model/diffTab";
import { useReviewUnread } from "../../review/store/useReviewUnread";
import type { CollapsedFiles } from "../../review/model/diffTab";
import type { LibraryPipelineEntry } from "../api/libraryPipelines";
import type { RunState } from "../../shared/generated/RunState";
import type { EditorPipelineDef } from "../model/pipeline";
import ExtensionDetailTab from "../../shared/extensions/ExtensionDetailTab";
import { useDetailTabs } from "../../shared/extensions/catalogue";

import { InfoTab } from "./InfoTab";
import { ManagerTab } from "./ManagerTab";
import { YamlTab } from "./YamlTab";
import VersionsTab from "./VersionsTab";
import { useEditStore } from "../store/editStore";
import { resolveInfoTab, type TabId } from "../../lib/infoPanelReconcile";

export type { TabId };

interface Props {
  run: RunState | null;
  pipeline: EditorPipelineDef | null;
  /** @deprecated Instance pipelines no longer have a library scope. */
  libraryPipelines?: LibraryPipelineEntry[];
  /** @deprecated Instance pipelines refresh through the edit store. */
  onLibraryChanged?: () => void;
  onClose: () => void;
  /** Uncontrolled: the tab the panel opens on (Info by default). */
  initialTab?: TabId;
  /** Controlled (#938): the requested tab, owned by the host — which lights the
   *  toolbar buttons from the same `resolveInfoTab`. Pair with `onTabChange`. */
  tab?: TabId;
  /** Called when the user picks a core tab (both modes). */
  onTabChange?: (tab: TabId) => void;
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
  tab,
  onTabChange,
  scrollToLine,
  assistantId,
  onRefreshRun,
  onOpenSettings,
}: Props) {
  const pipelineName = run?.pipeline_name ?? pipeline?.name ?? "Untitled";
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
  // ADR-0088: the history of the registry pipeline the active tab shows (its
  // draft, or one of its versions read only).
  const versionsId = useEditStore((s) => {
    const active = s.openTabs.find((t) => t.id === s.activeTabId);
    if (!active || active.runId != null) return null;
    return active.version?.pipelineId ?? active.id;
  });
  const hasVersions = !run && versionsId != null;
  const [ownTab, setOwnTab] = useState<TabId>(initialTab ?? "info");
  const resolvedTab = resolveInfoTab(tab ?? ownTab, { hasRun: run != null, hasAssistant, hasVersions });

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
  // ADR-0081: the tabs extensions add to a Run's detail (`run/detail`), after the core's.
  const runEntity = useMemo(
    () => (run ? { kind: "run", id: run.run_id, attributes: run.attributes ?? {} } : null),
    [run],
  );
  const { tabs: extensionTabs, context: extensionTabContext } = useDetailTabs(runEntity);
  const [extensionTab, setExtensionTab] = useState<string | null>(null);
  const activeExtension = run ? (extensionTabs.find((t) => t.key === extensionTab) ?? null) : null;
  const shownTab = activeExtension ? null : resolvedTab;
  const selectTab = (id: TabId) => {
    if (id === "diff" || resolvedTab === "diff") setSeenDeliverySig(deliverySig);
    if (tab === undefined) setOwnTab(id);
    onTabChange?.(id);
    setExtensionTab(null);
  };
  const nudgeDiff =
    run != null && shownTab !== "diff" && seenDeliverySig !== deliverySig && deliverySig !== "";
  // #751: sent review comments with an unread reply — a solid count pill that
  // replaces the blue dot while > 0 (a number beats a dot; both mean "come look").
  // Cleared by opening the Review page (per browser, localStorage).
  const unreadReplies = useReviewUnread(run);

  // The quiet nudge (manager on demand): the exact moment a manager helps most
  // is when the Run is parked on the user — an amber dot on the tab points it
  // out without banners or auto-switching.
  const nudgeManager = run?.status === "awaiting_user" && !(run.has_manager ?? false);

  const core: (InfoTabItem & { show: boolean })[] = [
    { key: "info", label: "Info", icon: Info, show: true },
    { key: "versions", label: "Versions", icon: History, show: hasVersions },
    {
      key: "diff",
      label: "Diff",
      icon: GitCompare,
      show: run != null,
      // #751: sent review comments with an unread reply — a count that replaces
      // the blue dot while > 0 (a number beats a dot; both mean "come look").
      count: unreadReplies > 0 ? unreadReplies : undefined,
      countTitle: `${unreadReplies} review comment${unreadReplies === 1 ? "" : "s"} with an unread reply`,
      countTestId: "diff-tab-unread",
      dot: unreadReplies === 0 && nudgeDiff ? "var(--color-st-running)" : undefined,
      dotTestId: "diff-tab-dot",
    },
    // #752 (closing the second half of #566): the Repositories view is a tab of
    // the Run panel, reachable on any selection, archived Runs included (frozen
    // list). No dot: nothing asynchronous happens to repositories.
    { key: "repositories", label: "Repos", title: "Repositories", icon: FolderGit2, show: run != null },
    {
      key: "manager",
      label: "Manager",
      icon: Bot,
      show: run != null,
      dot: nudgeManager ? "var(--color-st-await)" : undefined,
      dotTestId: "manager-tab-dot",
    },
    { key: "assistant", label: "Assistant", icon: Sparkles, show: hasAssistant },
    { key: "yaml", label: "YAML", icon: FileCode, show: true },
  ];
  // ADR-0081: the tabs extensions add to a Run's detail, after the core's.
  const items: InfoTabItem[] = [
    ...core.filter((t) => t.show),
    ...(run
      ? extensionTabs.map((t) => ({ key: `ext-${t.key}`, label: t.contribution.label, icon: Puzzle }))
      : []),
  ];
  const activeKey = activeExtension ? `ext-${activeExtension.key}` : (shownTab ?? "info");

  return (
    <aside className="ei" data-testid="pipeline-info-panel">
      <div className="ei-bar">
        <InfoTabs
          items={items}
          value={activeKey}
          onChange={(key) => {
            if (key.startsWith("ext-")) setExtensionTab(key.slice(4));
            else selectTab(key as TabId);
          }}
        />
        <span className="ei-bar__actions">
          <IconButton icon={X} label="Close pipeline info" kbd="Esc" onClick={onClose} data-testid="info-panel-close" />
        </span>
      </div>

      {activeExtension && run && (
        <ExtensionDetailTab
          key={`${activeExtension.key}:${run.run_id}`}
          tab={activeExtension}
          context={extensionTabContext}
        />
      )}

      {shownTab === "info" && (
        <InfoTab
          run={run}
          pipeline={pipeline}
          pipelineName={pipelineName}
          pipelineId={run ? null : (assistantId ?? null)}
          hasAssistant={hasAssistant}
          onOpenDiff={() => selectTab("diff")}
          onClose={onClose}
        />
      )}

      {shownTab === "versions" && hasVersions && versionsId && (
        <VersionsTab key={versionsId} pipelineId={versionsId} />
      )}

      {shownTab === "diff" && run && (
        <DiffTab
          key={run.run_id}
          run={run}
          collapsed={diffCollapsed}
          onCollapsedChange={onDiffCollapsedChange}
        />
      )}

      {shownTab === "repositories" && run && (
        <div data-testid="repositories-tab" className="pi-body">
          {run.target_repo ? (
            <RepositoriesSection key={run.run_id} run={run} onEdited={onRefreshRun} />
          ) : (
            <div data-testid="repositories-tab-empty" title="The Run works in the daemon's own checkout">
              <EmptyState icon={FolderGit2}>No target repository</EmptyState>
            </div>
          )}
        </div>
      )}

      {shownTab === "manager" && run && managerSession && (
        <ManagerTab
          key={run.run_id}
          run={run}
          session={managerSession}
          onRefreshRun={onRefreshRun}
          onOpenSettings={onOpenSettings}
        />
      )}

      {shownTab === "assistant" && hasAssistant && assistantId && (
        // #594: NOT keyed on the pipeline. One shared assistant means switching
        // template must keep the same session and the same conversation — a
        // remount per pipeline is the exact opposite of sharing.
        <AssistantPanel />
      )}

      {shownTab === "yaml" && (
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
