import { useCallback, useEffect, useState } from "react";
import { useDaemonSocket } from "../run/store/useDaemonSocket";
import { useResizableLayout } from "../hooks/useResizableLayout";
import { useLibrary } from "../hooks/useLibrary";
import { useSelectedRun } from "../run/store/useSelectedRun";
import { useDaemonSessions } from "../session/store/useDaemonSessions";
import { useRightPaneRouter } from "../hooks/useRightPaneRouter";
import { useLibassistLifecycle } from "../session/store/useLibassistLifecycle";
import { useTerminalOverlayOpen } from "../session/store/terminalOverlayStore";
import UnifiedLeftPanel from "./UnifiedLeftPanel";
import NewRunModal from "../run/components/NewRunModal";
import { useUpdateStatus } from "../platform/store/useUpdateStatus";
import { useUpdateFlow } from "../platform/store/useUpdateFlow";
import TopBar from "./TopBar";
import { useTriggers } from "../trigger/store/useTriggers";
import { useTriggersPause } from "../trigger/store/useTriggersPause";
import { TooltipProvider } from "../shared/ui/tooltip";
import Toaster from "../shared/ui/Toaster";
import ExtensionContextProvider from "../shared/extensions/ExtensionContextProvider";
import { useEditStore } from "../pipeline/store/editStore";
import TourHost from "./tour/TourHost";
import {
  ResizablePanelGroup,
  ResizablePanel,
  ResizableHandle,
} from "../shared/ui/resizable";
import { useProjects, useRuns } from "./useAppData";
import { useSurfaces } from "./useSurfaces";
import { useAppTour } from "./useAppTour";
import { useRunNavigation } from "./useRunNavigation";
import { useRunLink } from "./useRunLink";
import { formatRunLink } from "../shared/extensions/deepLink";
import { setSignInReturnLink } from "../shared/http/signIn";
import { useEditorHotkeys } from "./useEditorHotkeys";
import { useLaunchActions } from "./useLaunchActions";
import { useShellRefresh } from "./useShellRefresh";
import { useDaemonEvents } from "./useDaemonEvents";
import { useInfoPanel } from "./useInfoPanel";
import { useCollapsibleRightPane } from "./useCollapsibleRightPane";
import CenterPane from "./CenterPane";
import RightPane from "./RightPane";
import AppSurfaces from "./AppSurfaces";
import StatusBar from "./StatusBar";
import UpdateModals from "./UpdateModals";
import EditorModals from "./EditorModals";
import RunNowErrorToast from "./RunNowErrorToast";

const PANEL_IDS = ["left", "center", "right"];
const DEFAULT_SIZES = { left: 15, center: 60, right: 25 };
/** #949: the right pane collapses on its own; the canvas takes its width meanwhile. */
const RIGHT_COLLAPSIBLE = { panelId: "right", absorbInto: "center" };

/** The app shell: the Runs, Triggers and Projets on the left, the canvas in the
 *  center, the inspector on the right, the status bar, and every global modal. */
export default function App() {
  const { status, subscribe } = useDaemonSocket();
  const { entries: libraryEntries, refresh: refreshLibrary } = useLibrary();
  const { runs, runsLoaded, refresh: refreshRuns } = useRuns();
  const { sessions, refresh: refreshSessions } = useDaemonSessions();
  // #697: the daemon's cached version-check state, for the status-bar badge.
  const { status: updateStatus, refresh: refreshUpdateStatus } = useUpdateStatus();
  const { triggers, refresh: refreshTriggers } = useTriggers();
  const { projects, refresh: refreshProjects } = useProjects();
  // #348: global Trigger pause, held here so the WS dispatcher can flip it live.
  const {
    paused: triggersPaused,
    setPaused: setTriggersPaused,
    toggle: handleTogglePause,
    hydrate: hydrateTriggersPause,
  } = useTriggersPause();
  const [selectedTriggerId, setSelectedTriggerId] = useState<string | null>(null);
  // #320: the tab id (=== pipeline_id) that selecting a Trigger opened in the
  // canvas. The reconciliation below reads it to tell the Trigger's OWN
  // openPipeline focus change apart from a genuine user canvas interaction.
  // State (not a ref) because the reconciliation reads it *during render*, and
  // it's always set alongside `selectedTriggerId` (which re-renders anyway), so
  // there's no extra render — while a ref read during render is a React
  // anti-pattern the compiler lint (`react-hooks/refs`) rejects.
  const [triggerOpenedTabId, setTriggerOpenedTabId] = useState<string | null>(null);
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const { run: selectedRun, select: selectRun, refresh: refreshRun } = useSelectedRun();
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const updates = useUpdateFlow(updateStatus, status);
  const { setChangelogOpen, setUpdateConfirmOpen } = updates;
  const surfaces = useSurfaces();
  const { openSettings, openStats } = surfaces;
  const { tour, showWelcome, setTourOffered, startTourById, startFullTour } = useAppTour(
    runs,
    runsLoaded,
    selectedRun,
  );
  // #341: bumped on every `trigger_fired` WS message so an open TriggerDetail
  // panel refetches its fire history (it otherwise only fetches on trigger.id).
  const [firesRefreshKey, setFiresRefreshKey] = useState(0);
  const selection = useEditStore((s) => s.selection);
  const openTabs = useEditStore((s) => s.openTabs);
  const editActiveTabId = useEditStore((s) => s.activeTabId);
  const editLastRekey = useEditStore((s) => s.lastRekey);
  // Merged /pipelines list — used to derive the selected trigger's prompt-required
  // signal for the guard dry-run caveat (#351).
  const pipelines = useEditStore((s) => s.pipelines);

  const editTab = openTabs.find((t) => t.id === editActiveTabId);
  const isEditingRun = editTab?.scope === "run";
  const hasEditTab = editTab != null;

  // #302 / ADR-0048: the Assistant authors a library *template*, so it targets the
  // active edit tab's pipeline id + scope — never a run. `null` on a run tab hides
  // the Assistant tab (the Manager tab covers a run instead).
  const assistantId = editTab && !isEditingRun ? editTab.id : null;
  // Paired with `assistantId`, deliberately: a scope of `"run"` alongside a null
  // id would be a half-fact, and it would churn the lifecycle effect on every
  // hop to a Run for nothing.
  const assistantScope = assistantId ? editTab?.scope : undefined;

  // #594 / ADR-0051 §4: the reap fires when the user leaves **every** edit view —
  // so it reads the open tabs, not the active one. Keying it on the active tab
  // (the first cut) meant a glance at a Run threw the conversation away while two
  // templates were still open, which is the regret this issue is made of.
  const hasTemplateTab = openTabs.some((t) => t.scope !== "run");

  // The assistant's whole lifecycle hangs here, and not down in the Assistant
  // tab. This is the right altitude for it: the tab unmounts on every panel close
  // (#385), the user does not stop editing when it does, and a lifecycle keyed on
  // the tab is what made the assistant lose its conversation on each round trip
  // between two templates.
  useLibassistLifecycle(assistantId, assistantScope, hasTemplateTab);

  // #315: an archived run is read-only — its worktree (and `pipeline.yaml`) is
  // gone, so any save would PUT into a 404. `isArchived` tracks the *selected*
  // run (drives the NodeDetailPanel + the archived aside below). The edit
  // affordances (Ctrl+S / undo-redo / the canvas) gate on the stricter
  // `isActiveRunArchived`: the ACTIVE tab must BE that archived run, so a
  // template tab stays editable while an archived run is merely selected.
  const isArchived = selectedRun?.status === "archived";
  const isActiveRunArchived =
    isEditingRun && editTab?.runId === selectedRun?.run_id && isArchived;
  const conflictTab = openTabs.find((t) => t.conflict != null);
  const saveErrorTab = openTabs.find((t) => t.saveError != null);
  // What the info panel offers (#938): a Run's tabs, or a template's Assistant.
  const infoHasRun = isEditingRun && selectedRun != null;
  const info = useInfoPanel(saveErrorTab, { hasRun: infoHasRun, hasAssistant: !infoHasRun && !!assistantId });
  const { infoPanelOpen, setInfoPanelOpen, handleCloseInfo } = info;

  // The Trigger backing the right-panel detail view (#162), the pane-owner
  // precedence (#247), the two render-time reconciliations (#320 canvas-reclaim,
  // #385 info auto-close), and the synthesized pending run-pane node all live in
  // useRightPaneRouter (#359). It runs the setState-during-render reconciliations
  // in-body here (App owns `selectedTriggerId`/`infoPanelOpen`, passed in with
  // their setters) so no stale frame of a shadowed panel is painted.
  const launch = useLaunchActions({
    triggers,
    triggersPaused,
    selectRun,
    setSelectedTriggerId,
    setSelectedRunId,
    setSelectedNodeId,
    setTriggerOpenedTabId,
  });
  const { openNewRunModal, handleCloseNewRunModal, runNowError, setRunNowError } = launch;

  const { paneOwner, selectedTrigger, triggerPromptRequired, runNode, collapsed: rightPaneCollapsed } =
    useRightPaneRouter({
      selection,
      editActiveTabId,
      tabRekey: editLastRekey,
      hasEditTab,
      isRunTab: isEditingRun,
      selectedNodeId,
      selectedTriggerId,
      setSelectedTriggerId,
      triggerOpenedTabId,
      infoPanelOpen,
      setInfoPanelOpen,
      triggers,
      pipelines,
      selectedRun,
    });

  useShellRefresh({
    status,
    selectedRun,
    refreshRuns,
    refreshSessions,
    refreshTriggers,
    refreshProjects,
    hydrateTriggersPause,
    refreshUpdateStatus,
  });

  const {
    orchestratorReturn,
    landOnOrchestration,
    handleSelectRun,
    handleOpenChildRun,
    handleBackToOrchestrator,
  } = useRunNavigation({
    selectedRun,
    selectRun,
    setSelectedTriggerId,
    setSelectedRunId,
    setSelectedNodeId,
  });

  // CORE-1: `#/runs/<id>` selects the Run, from the address bar or an extension.
  useRunLink(handleSelectRun);
  // CORE-2: the address keeps no Run, so a lost session comes back to it this way.
  useEffect(() => {
    setSignInReturnLink(selectedRunId ? formatRunLink(selectedRunId) : null);
    return () => setSignInReturnLink(null);
  }, [selectedRunId]);

  const handleRunCreated = useCallback(
    (runId: string) => {
      refreshRuns();
      handleSelectRun(runId);
    },
    [refreshRuns, handleSelectRun],
  );

  useEditorHotkeys(hasEditTab, isActiveRunArchived);

  const bumpFiresRefreshKey = useCallback(() => setFiresRefreshKey((v) => v + 1), []);
  useDaemonEvents({
    subscribe,
    selectedTriggerId,
    refreshRuns,
    refreshRun,
    refreshSessions,
    refreshTriggers,
    refreshProjects,
    setTriggersPaused,
    bumpFiresRefreshKey,
  });

  const layout = useResizableLayout("run", PANEL_IDS, DEFAULT_SIZES, RIGHT_COLLAPSIBLE);
  // #44: an enlarged node terminal covers the app, its panels stop resizing.
  const terminalOverlayOpen = useTerminalOverlayOpen();
  const minSizePx = `${layout.minSizePx}px`;
  const rightPane = useCollapsibleRightPane(rightPaneCollapsed, layout.persistedSize, DEFAULT_SIZES.right);

  return (
    <TooltipProvider>
    {/* ADR-0081 #29: every extension contribution receives the selected Run and its Projet. */}
    <ExtensionContextProvider selectedRun={selectedRun} runs={runs} projects={projects}>
    <div className="flex h-full flex-col bg-bg-1 text-fg">
      <TopBar
        onOpenSettings={() => openSettings()}
        onOpenStats={() => openStats()}
      />
      <main className="min-h-0 flex-1">
        <ResizablePanelGroup
          orientation="horizontal"
          defaultLayout={layout.defaultLayout}
          onLayoutChanged={layout.onLayoutChanged}
          // #44: no resizing under an enlarged node terminal.
          disabled={terminalOverlayOpen}
        >
          <ResizablePanel defaultSize={layout.defaultLayout.left} minSize={minSizePx} id="left">
            <UnifiedLeftPanel
              runs={runs}
              selectedRunId={selectedRunId}
              onSelectRun={handleSelectRun}
              onNewRun={() => openNewRunModal({ kind: "run" })}
              triggers={triggers}
              selectedTriggerId={selectedTriggerId}
              onSelectTrigger={launch.handleSelectTrigger}
              onNewTrigger={() => openNewRunModal({ kind: "new-trigger" })}
              onTriggersChanged={refreshTriggers}
              onRunNowTrigger={launch.handleRunNowTrigger}
              onEditTrigger={(t) => openNewRunModal({ kind: "edit-trigger", trigger: t })}
              triggersPaused={triggersPaused}
              onTogglePause={handleTogglePause}
              projects={projects}
              onProjectsChanged={refreshProjects}
            />
          </ResizablePanel>

          <ResizableHandle />

          <ResizablePanel defaultSize={layout.defaultLayout.center} id="center">
            <CenterPane
              hasEditTab={hasEditTab}
              orchestratorReturn={orchestratorReturn}
              onBackToOrchestrator={handleBackToOrchestrator}
              libraryEntries={libraryEntries}
              refreshLibrary={refreshLibrary}
              infoPanelOpen={info.infoActive}
              assistantActive={info.assistantActive}
              onToggleInfo={info.handleToggleInfo}
              onCloseInfo={handleCloseInfo}
              onOpenAssistant={info.handleToggleAssistant}
              selectedRun={selectedRun}
              onSelectRun={handleSelectRun}
              rightPaneCollapsed={rightPaneCollapsed}
            />
          </ResizablePanel>

          {/* #949: no handle while collapsed — the pane cannot be dragged out.
              `invisible`, not `hidden`: a display:none separator has an empty
              rect, which breaks the library's hit regions for the OTHER handle. */}
          <ResizableHandle
            id="right-pane-handle"
            disabled={rightPaneCollapsed}
            className={rightPaneCollapsed ? "invisible" : undefined}
          />

          <ResizablePanel
            defaultSize={layout.defaultLayout.right}
            minSize={minSizePx}
            collapsible={rightPane.collapsible}
            panelRef={rightPane.panelRef}
            id="right"
            className="panel-r"
            data-collapsed={rightPaneCollapsed || undefined}
          >
            <RightPane
              paneOwner={paneOwner}
              selectedTrigger={selectedTrigger}
              triggerPromptRequired={triggerPromptRequired}
              firesRefreshKey={firesRefreshKey}
              runNode={runNode}
              editTab={editTab}
              selectedRun={selectedRun}
              selectedNodeId={selectedNodeId}
              isArchived={isArchived}
              isActiveRunArchived={isActiveRunArchived}
              assistantId={assistantId}
              infoPanelTab={info.infoPanelTab}
              onInfoPanelTabChange={info.setInfoPanelTab}
              infoPanelScrollToLine={info.infoPanelScrollToLine}
              onCloseInfo={handleCloseInfo}
              libraryEntries={libraryEntries}
              onLibraryChanged={refreshLibrary}
              landOnOrchestration={landOnOrchestration}
              onSelectRun={handleSelectRun}
              onOpenChildRun={handleOpenChildRun}
              onRefreshRun={refreshRun}
              openSettings={openSettings}
            />
          </ResizablePanel>
        </ResizablePanelGroup>
      </main>
      <StatusBar
        status={status}
        sessions={sessions}
        update={updateStatus}
        onOpenVersion={() => setChangelogOpen(true)}
      />
      <NewRunModal
        open={launch.newRunModalOpen}
        onClose={handleCloseNewRunModal}
        onCreated={(runId) => {
          handleCloseNewRunModal();
          handleRunCreated(runId);
        }}
        openIntent={launch.openIntent}
        onTriggerSaved={refreshTriggers}
        // #691: the one programmatic door into Settings from New Run — lands on the section,
        // pulses it once; New Run stays mounted underneath.
        onManageStagingProfiles={() =>
          openSettings({ category: "sandbox", section: "staging-profiles" })
        }
        // #824: the one thing a tour cannot do by observing — the training repo is
        // not in recents (nothing has run in it), so the step that says "click the
        // magnifier" also says where it opens. Undefined outside a tour.
        repoExplorerStartPath={tour.view?.step?.explorerStart}
      />
      <AppSurfaces
        surfaces={surfaces}
        liveSessions={sessions.live}
        onSessionsSaved={refreshSessions}
        onOpenChangelog={() => setChangelogOpen(true)}
        onRequestUpdate={() => setUpdateConfirmOpen(true)}
        onStartTour={startTourById}
        onStartFullTour={startFullTour}
      />
      {/* After the full-window surfaces, which it must paint over. */}
      <UpdateModals updateStatus={updateStatus} flow={updates} openSettings={openSettings} />
      <EditorModals
        conflictTab={conflictTab}
        saveErrorTab={saveErrorTab}
        onViewYaml={info.handleViewYaml}
      />
      {/* #823 — the Projecteur, the tour popover, the welcome modal and the two
          end cards. Mounted last so it paints over every surface above. */}
      <TourHost
        controller={tour}
        showWelcome={showWelcome}
        onWelcomeAnswered={() => setTourOffered(true)}
        onOpenTutorials={() => openSettings({ category: "general", section: "tutorials" })}
      />
      {runNowError && <RunNowErrorToast error={runNowError} onDismiss={() => setRunNowError(null)} />}
      <Toaster />
    </div>
    </ExtensionContextProvider>
    </TooltipProvider>
  );
}
