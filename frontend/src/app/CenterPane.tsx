import { ArrowLeft, Play, Workflow } from "lucide-react";
import EditCanvas from "../pipeline/components/EditCanvas";
import TabBar from "../pipeline/components/TabBar";
import MountedPageView from "../platform/components/MountedPageView";
import { useActivePage, useMountedPages } from "../platform/store/mountedPages";
import type { LibraryEntry } from "../pipeline/model/library";
import type { RunState } from "../shared/generated/RunState";
import type { RunListEntry } from "../shared/generated/RunListEntry";
import { Button, EmptyState, IconButton } from "../shared/ui/kit";
import { canCreateSomeRun } from "../run/model/runCreationRights";
import { useRunCreationRightsStore } from "../run/store/runCreationRightsStore";
import type { Project } from "../workspace/model/project";
import type { useRunNavigation } from "./useRunNavigation";

/**
 * The center of the shell: the tab bar, the way back to an orchestrator node
 * (#723) and the canvas of the active tab — or, with no document open, an
 * icon, three words and New run. A
 * mounted page's tab, when shown, covers the canvas, which stays mounted
 * underneath so its viewport and sessions survive the switch.
 */
export default function CenterPane({
  hasEditTab,
  orchestratorReturn,
  onBackToOrchestrator: handleBackToOrchestrator,
  libraryEntries,
  refreshLibrary,
  infoPanelOpen,
  assistantActive,
  onToggleInfo: handleToggleInfo,
  onCloseInfo: handleCloseInfo,
  onOpenAssistant: handleOpenAssistant,
  selectedRun,
  onSelectRun: handleSelectRun,
  rightPaneCollapsed,
  projects,
  runs = [],
  onNewRun,
}: {
  hasEditTab: boolean;
  orchestratorReturn: ReturnType<typeof useRunNavigation>["orchestratorReturn"];
  onBackToOrchestrator: () => void;
  libraryEntries: LibraryEntry[];
  refreshLibrary: () => void;
  infoPanelOpen: boolean;
  assistantActive: boolean;
  onToggleInfo: () => void;
  onCloseInfo: () => void;
  onOpenAssistant: () => void;
  selectedRun: RunState | null;
  onSelectRun: (runId: string) => void;
  /** #949: whether the right pane is folded — the canvas re-frames on its toggle. */
  rightPaneCollapsed: boolean;
  /** The Projets the tab bar's validation indicator names. */
  projects: Project[];
  /** The Runs, for the name and status of their tabs. */
  runs?: RunListEntry[];
  /** The empty centre's action; absent, none. */
  onNewRun?: () => void;
}) {
  const pageTabCount = useMountedPages((s) => s.tabs.length);
  const activePage = useActivePage();
  // CORE-5: no New run where no Projet allows creating a Run.
  const canCreate = useRunCreationRightsStore((s) => canCreateSomeRun(s.rights));
  const placeholder = (
    <div className="sh-center-empty" data-testid="center-empty">
      <EmptyState
        icon={Workflow}
        action={
          onNewRun && canCreate && (
            <Button icon={Play} onClick={onNewRun} data-testid="center-new-run">
              New run
            </Button>
          )
        }
      >
        No document open
      </EmptyState>
    </div>
  );
  return hasEditTab || pageTabCount > 0 ? (
    <div className="flex h-full min-w-0 flex-col">
      <TabBar projects={projects} runs={runs} />
      {activePage && <MountedPageView page={activePage} />}
      {!hasEditTab && !activePage && placeholder}
      {hasEditTab && (
        <div hidden={activePage != null} className="flex min-h-0 flex-1 flex-col">
          {orchestratorReturn && selectedRun?.run_id === orchestratorReturn.childRunId && (
            <div className="cv-child" data-testid="orchestrator-back-bar">
              <IconButton
                icon={ArrowLeft}
                size="sm"
                label="Back to parent run"
                data-testid="orchestrator-back"
                onClick={handleBackToOrchestrator}
              />
              <span className="cv-child__parent" title={orchestratorReturn.parentRunName}>
                {orchestratorReturn.parentRunName}
              </span>
              <span className="cv-child__node" title={orchestratorReturn.nodeName}>
                {orchestratorReturn.nodeName}
              </span>
            </div>
          )}
          <EditCanvas
            libraryEntries={libraryEntries}
            onLibraryDelete={async (name) => {
              const { deleteFromLibrary: delLib } = await import("../pipeline/api/library");
              await delLib(name);
              refreshLibrary();
            }}
            infoOpen={infoPanelOpen}
            onToggleInfo={handleToggleInfo}
            onCloseInfo={handleCloseInfo}
            assistantActive={assistantActive}
            onOpenAssistant={handleOpenAssistant}
            runState={selectedRun}
            onSelectRun={handleSelectRun}
            rightPaneCollapsed={rightPaneCollapsed}
          />
        </div>
      )}
    </div>
  ) : (
    placeholder
  );
}
