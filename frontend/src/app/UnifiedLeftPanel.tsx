import { useEffect, useState } from "react";
import { CalendarClock, Play, Workflow } from "lucide-react";
import type { RunListEntry } from "../shared/generated/RunListEntry";
import type { Trigger } from "../shared/generated/Trigger";
import type { TriggerListEntry } from "../shared/generated/TriggerListEntry";
import type { Project } from "../workspace/model/project";
import type { ProjectsEntry } from "../workspace/model/projectsSettings";
import type { DeletedProject } from "../shared/generated/DeletedProject";
import type { LibraryPipelineEntry } from "../pipeline/api/libraryPipelines";
import { useEditStore } from "../pipeline/store/editStore";
import { useSelectionStore } from "../stores/selectionStore";
import RunsPane from "../run/components/runsList/RunsPane";
import RunsModals from "../run/components/runsList/RunsModals";
import { useRunsPane } from "../run/store/runsList/useRunsPane";
import TriggersListPanel from "../trigger/components/TriggersListPanel";
import PipelinesPane from "../pipeline/components/PipelinesPane";
import { LeftTabsProvider, type LeftTab as LeftTabItem } from "../shared/ui/leftTabs";

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
  /** The deleted Projets the Runs still name (CORE-96). */
  deletedProjects?: DeletedProject[];
  /** CORE-96: a group header's pencil opens Settings › Projects there. */
  onOpenProjectSettings?: (entry?: ProjectsEntry) => void;
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
  deletedProjects,
  onOpenProjectSettings,
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
  const runsPane = useRunsPane({
    runs,
    selectedRunId,
    onSelectRun,
    projects,
    deletedProjects,
    onOpenProjectSettings,
  });

  // #577 — per-tab selection counts, for the dot left on the tab you switch away
  // from (so an in-flight selection is never silently lost).
  const tabs: LeftTabItem[] = [
    { id: "runs", label: "Runs", icon: Play, selected: runSelIds.length },
    { id: "triggers", label: "Triggers", icon: CalendarClock, selected: triggerSelCount },
    { id: "library", label: "Pipelines", icon: Workflow, selected: librarySelIds.length },
  ];

  return (
    <aside className="flex h-full min-w-0 flex-col bg-surface-1">
      {/* The header of each pane draws the tab strip, its title and its actions. */}
      <LeftTabsProvider value={{ tabs, active: activeTab, onChange: (id) => setActiveTab(id as LeftTab) }}>
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
            onOpenProjectSettings={onOpenProjectSettings}
          />
        </div>
      )}

      {/* Library pane */}
      {activeTab === "library" && <PipelinesPane projects={projects} />}

      </LeftTabsProvider>
      <RunsModals pane={runsPane} runs={runs} />
    </aside>
  );
}
