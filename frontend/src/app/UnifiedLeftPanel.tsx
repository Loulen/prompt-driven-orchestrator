import { useEffect, useState } from "react";
import type { RunListEntry } from "../shared/generated/RunListEntry";
import type { Trigger } from "../shared/generated/Trigger";
import type { TriggerListEntry } from "../shared/generated/TriggerListEntry";
import type { Project } from "../workspace/model/project";
import type { LibraryPipelineEntry } from "../pipeline/api/libraryPipelines";
import { useEditStore } from "../pipeline/store/editStore";
import { useSelectionStore } from "../stores/selectionStore";
import RunsPane from "../run/components/runsList/RunsPane";
import RunsModals from "../run/components/runsList/RunsModals";
import { useRunsPane } from "../run/store/runsList/useRunsPane";
import TriggersListPanel from "../trigger/components/TriggersListPanel";
import PipelinesPane from "../pipeline/components/PipelinesPane";

type LeftTab = "runs" | "triggers" | "library";

interface Props {
  runs: RunListEntry[];
  selectedRunId: string | null;
  onSelectRun: (runId: string) => void;
  onNewRun: () => void;
  /** @deprecated Instance pipelines no longer have a library scope. */
  libraryPipelines?: LibraryPipelineEntry[];
  /** @deprecated Instance pipelines refresh through loadPipelines. */
  onLibraryPipelinesChanged?: () => void;
  /** Triggers (#160). Optional so existing callers/tests keep working. */
  triggers?: TriggerListEntry[];
  selectedTriggerId?: string | null;
  onSelectTrigger?: (triggerId: string) => void;
  onNewTrigger?: () => void;
  onTriggersChanged?: () => void;
  /** Run-now / edit a Trigger via the New Run modal (#162). */
  onRunNowTrigger?: (trigger: Trigger) => void;
  onEditTrigger?: (trigger: Trigger) => void;
  /** #348 global Trigger kill-switch state + toggle. Optional so existing
   * callers/tests keep working (defaults to not-paused, no-op toggle). */
  triggersPaused?: boolean;
  onTogglePause?: () => void;
  /** Projets (#552) — the group-by-Projet layer and the pencil's source of
   *  truth. Optional so existing callers/tests keep working (no Projet → the
   *  #258 per-path grouping, unchanged). */
  projects?: Project[];
  onProjectsChanged?: () => void;
}

export default function UnifiedLeftPanel({
  runs,
  selectedRunId,
  onSelectRun,
  onNewRun,
  triggers = [],
  selectedTriggerId = null,
  onSelectTrigger,
  onNewTrigger,
  onTriggersChanged,
  onRunNowTrigger,
  onEditTrigger,
  triggersPaused = false,
  onTogglePause,
  projects = [],
  onProjectsChanged,
}: Props) {
  const [activeTab, setActiveTab] = useState<LeftTab>("runs");

  const loadPipelines = useEditStore((s) => s.loadPipelines);

  // #577 — multi-select. The per-tab sets live in the shared store (so a tab's
  // count survives a switch as its badge); the Runs tab reads its own set.
  const runSelIds = useSelectionStore((s) => s.runs);
  const librarySelIds = useSelectionStore((s) => s.library);
  const triggerSelCount = useSelectionStore((s) => s.triggers.length);

  useEffect(() => {
    loadPipelines();
  }, [loadPipelines]);

  // The Runs tab's state (list, filters, tree, selection, confirms) — kept here,
  // above the tab switch, so it survives switching tabs.
  const runsPane = useRunsPane({ runs, selectedRunId, onSelectRun, projects });

  const tabs: { id: LeftTab; label: string }[] = [
    { id: "runs", label: "Runs" },
    { id: "triggers", label: "Triggers" },
    { id: "library", label: "Pipelines" },
  ];
  // #577 — per-tab selection counts, for the badge left on the tab you switch away
  // from (so an in-flight selection is never silently lost).
  const selCounts: Record<LeftTab, number> = {
    runs: runSelIds.length,
    triggers: triggerSelCount,
    library: librarySelIds.length,
  };

  return (
    <aside className="flex h-full flex-col bg-bg-2">
      {/* Three-tab strip: Runs · Triggers · Library (#160) */}
      <div role="tablist" className="flex h-[36px] shrink-0 items-stretch border-b border-line">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            role="tab"
            aria-selected={activeTab === tab.id}
            // #823: a tour has to be able to aim at a tab — its first step opens
            // the one the rest of the tour lives in.
            data-testid={`left-tab-${tab.id}`}
            onClick={() => setActiveTab(tab.id)}
            className={`flex flex-1 cursor-pointer items-center justify-center gap-1.5 border-b-2 font-medium transition-colors ${
              activeTab === tab.id
                ? "border-acc text-fg"
                : "border-transparent text-fg-4 hover:text-fg-2"
            }`}
            style={{ fontSize: "11.5px" }}
          >
            {tab.label}
            {/* #577 — count badge left on a NON-active tab with a live selection. */}
            {activeTab !== tab.id && selCounts[tab.id] > 0 && (
              <span
                data-testid={`tab-badge-${tab.id}`}
                className="rounded-full bg-acc px-1.5 font-semibold text-on-acc"
                style={{ fontSize: "9px" }}
              >
                {selCounts[tab.id]}
              </span>
            )}
          </button>
        ))}
      </div>

      {/* Runs pane */}
      {activeTab === "runs" && (
        <RunsPane
          pane={runsPane}
          runs={runs}
          triggers={triggers}
          selectedRunId={selectedRunId}
          onSelectRun={onSelectRun}
          onNewRun={onNewRun}
          onSelectTrigger={onSelectTrigger}
          onShowTriggers={() => setActiveTab("triggers")}
        />
      )}

      {/* Triggers pane (#160) */}
      {activeTab === "triggers" && (
        <div className="min-h-0 flex-1" role="tabpanel">
          <TriggersListPanel
            triggers={triggers}
            selectedTriggerId={selectedTriggerId}
            onSelectTrigger={onSelectTrigger ?? (() => {})}
            onNewTrigger={onNewTrigger ?? (() => {})}
            onTriggersChanged={onTriggersChanged ?? (() => {})}
            onRunNow={onRunNowTrigger}
            onEditTrigger={onEditTrigger}
            paused={triggersPaused}
            onTogglePause={onTogglePause ?? (() => {})}
            projects={projects}
            onProjectsChanged={onProjectsChanged}
          />
        </div>
      )}

      {/* Library pane */}
      {activeTab === "library" && <PipelinesPane projects={projects} />}

      <RunsModals
        pane={runsPane}
        runs={runs}
        projects={projects}
        onProjectsChanged={onProjectsChanged}
      />
    </aside>
  );
}
