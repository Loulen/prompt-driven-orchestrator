import { ArrowLeft, GitFork } from "lucide-react";
import EditCanvas from "../pipeline/components/EditCanvas";
import TabBar from "../pipeline/components/TabBar";
import MountedPageView from "../platform/components/MountedPageView";
import { useActivePage, useMountedPages } from "../platform/store/mountedPages";
import type { LibraryEntry } from "../pipeline/model/library";
import type { RunState } from "../shared/generated/RunState";
import type { Project } from "../workspace/model/project";
import type { useRunNavigation } from "./useRunNavigation";

/**
 * The center of the shell: the tab bar, the way back to an orchestrator node
 * (#723) and the canvas of the active tab — or the invitation to open one. A
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
}) {
  const pageTabCount = useMountedPages((s) => s.tabs.length);
  const activePage = useActivePage();
  const placeholder = (
    <div className="flex h-full items-center justify-center text-fg-4" style={{ fontSize: "12px" }}>
      Select a run or open a pipeline to get started
    </div>
  );
  return hasEditTab || pageTabCount > 0 ? (
    <div className="flex h-full min-w-0 flex-col">
      <TabBar projects={projects} />
      {activePage && <MountedPageView page={activePage} />}
      {!hasEditTab && !activePage && placeholder}
      {hasEditTab && (
        <div hidden={activePage != null} className="flex min-h-0 flex-1 flex-col">
          {orchestratorReturn && selectedRun?.run_id === orchestratorReturn.childRunId && (
            <button
              type="button"
              data-testid="orchestrator-back-bar"
              onClick={handleBackToOrchestrator}
              className="flex shrink-0 cursor-pointer items-center gap-1.5 border-b border-line bg-bg-2 px-3 py-1 text-fg-3 transition-colors hover:text-fg"
              style={{ fontSize: "11px" }}
              title="Back to the orchestrator node, on its Orchestration tab"
            >
              <ArrowLeft size={12} />
              <span>
                Back to <span className="font-medium text-fg-2">{orchestratorReturn.nodeName}</span>
                <span className="text-fg-4"> · {orchestratorReturn.parentRunName}</span>
              </span>
              <span className="ml-auto flex items-center gap-1 text-fg-4" style={{ fontSize: "10px" }}>
                <GitFork size={10} /> child run
              </span>
            </button>
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
