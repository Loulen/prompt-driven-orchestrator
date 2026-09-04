import { useCallback, useEffect, useRef, useState } from "react";
import { Settings, BarChart3 } from "lucide-react";
import { useDaemonSocket } from "./hooks/useDaemonSocket";
import type { ConnectionStatus } from "./hooks/useDaemonSocket";
import { useResizableLayout } from "./hooks/useResizableLayout";
import { useLibrary } from "./hooks/useLibrary";
import { fetchRuns, fetchRun, fetchTriggers, fetchProjects, fetchSessions, fetchTriggersHealth, pauseTriggers } from "./api";
import { pickLatestLiveNode } from "./lib/pickLatestLiveNode";
import { useRightPaneRouter } from "./hooks/useRightPaneRouter";
import { useLibassistLifecycle } from "./hooks/useLibassistLifecycle";
import type { RunListEntry, RunState, Trigger, Project, DaemonStatus } from "./types";
import { shouldAutoSnapToLiveNode } from "./lib/autoSnap";
import SessionCounter from "./components/SessionCounter";
import ServiceHealthIndicator from "./components/ServiceHealthIndicator";
import UnifiedLeftPanel from "./components/UnifiedLeftPanel";
import NodeDetailPanel from "./components/NodeDetailPanel";
import RunInfoSidebar from "./components/RunInfoSidebar";
import NewRunModal, { RUN_INTENT } from "./components/NewRunModal";
import SettingsModal from "./components/SettingsModal";
import StatsModal from "./components/StatsModal";
import ConflictModal from "./components/ConflictModal";
import SaveErrorModal from "./components/SaveErrorModal";
import ConfirmCloseTabsModal from "./components/ConfirmCloseTabsModal";
import { useRecentReposStore } from "./stores/recentReposStore";
import type { TabId } from "./components/PipelineInfoPanel";
import EditCanvas from "./components/EditCanvas";
import TabBar from "./components/TabBar";
import NodeInspector from "./components/NodeInspector";
import MergeInspector from "./components/MergeInspector";
import PipelineInspector from "./components/PipelineInspector";
import PipelineInfoPanel from "./components/PipelineInfoPanel";
import StartInspector from "./components/StartInspector";
import EndInspector from "./components/EndInspector";
import MarkerInspector from "./components/MarkerInspector";
import { resolveNodeInspector } from "./lib/structuralMarkers";
import EdgeDetailPanel from "./components/EdgeDetailPanel";
import RegionInspector from "./components/RegionInspector";
import NoteInspector from "./components/NoteInspector";
import TriggerDetailPanel from "./components/TriggerDetailPanel";
import type { OpenIntent } from "./components/NewRunModal";
import { deriveEdgeTrigger } from "./lib/edgeTrigger";
import { handleUndoRedoKeydown } from "./lib/undoRedoHotkeys";
import InspectorTabs from "./components/InspectorTabs";
import { useInspectorTab } from "./hooks/useInspectorTab";
import { TooltipProvider } from "./components/ui/tooltip";
import { useEditStore } from "./stores/editStore";
import {
  ResizablePanelGroup,
  ResizablePanel,
  ResizableHandle,
} from "./components/ui/resizable";

const PANEL_IDS = ["left", "center", "right"];
const DEFAULT_SIZES = { left: 15, center: 60, right: 25 };

function useRuns() {
  const [runs, setRuns] = useState<RunListEntry[]>([]);

  const refresh = useCallback(async () => {
    try {
      setRuns(await fetchRuns());
    } catch {
      // ignore
    }
  }, []);

  return { runs, refresh };
}

function useSessions() {
  const [sessions, setSessions] = useState<DaemonStatus>({ live: 0, cap: 0 });

  const refresh = useCallback(async () => {
    try {
      setSessions(await fetchSessions());
    } catch {
      // ignore
    }
  }, []);

  return { sessions, refresh };
}

function useTriggers() {
  const [triggers, setTriggers] = useState<Trigger[]>([]);

  const refresh = useCallback(async () => {
    try {
      setTriggers(await fetchTriggers());
    } catch {
      // ignore
    }
  }, []);

  return { triggers, refresh };
}

// #552 — Projets, hydrated on mount and refreshed on a `project_changed` WS push
// (the daemon broadcasts one on every project mutation). Mirror of useTriggers.
function useProjects() {
  const [projects, setProjects] = useState<Project[]>([]);

  const refresh = useCallback(async () => {
    try {
      setProjects(await fetchProjects());
    } catch {
      // ignore
    }
  }, []);

  return { projects, refresh };
}

function useSelectedRun() {
  const [run, setRun] = useState<RunState | null>(null);
  const currentIdRef = useRef<string | null>(null);

  const refresh = useCallback(async () => {
    const id = currentIdRef.current;
    if (!id) return;
    try {
      const data = await fetchRun(id);
      if (currentIdRef.current === id) setRun(data);
    } catch {
      // ignore
    }
  }, []);

  const select = useCallback(
    (newId: string | null) => {
      currentIdRef.current = newId;
      if (!newId) {
        setRun(null);
        return;
      }
      fetchRun(newId)
        .then((data) => {
          if (currentIdRef.current === newId) setRun(data);
        })
        .catch(() => {});
    },
    [],
  );

  return { run, select, refresh };
}

export default function App() {
  const { status, subscribe } = useDaemonSocket();
  const { entries: libraryEntries, refresh: refreshLibrary } = useLibrary();
  const { runs, refresh: refreshRuns } = useRuns();
  const { sessions, refresh: refreshSessions } = useSessions();
  const { triggers, refresh: refreshTriggers } = useTriggers();
  const { projects, refresh: refreshProjects } = useProjects();
  // #348: global Trigger pause. Lifted here (not in a per-panel hook) so the WS
  // dispatcher can flip it live and every consumer stays in sync; hydrated on
  // mount from GET /triggers/health since there is no trigger polling.
  const [triggersPaused, setTriggersPaused] = useState(false);
  const [selectedTriggerId, setSelectedTriggerId] = useState<string | null>(null);
  // #368: mirror readable inside the stable WS callback (App.tsx subscribe
  // effect) without widening its deps — same latest-value idiom as
  // currentIdRef in useSelectedRun. A ref read during render is banned by
  // react-hooks/refs (see note ~L148), but here read/write are outside render.
  const selectedTriggerIdRef = useRef<string | null>(null);
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
  const [newRunModalOpen, setNewRunModalOpen] = useState(false);
  const [settingsModalOpen, setSettingsModalOpen] = useState(false);
  const [statsModalOpen, setStatsModalOpen] = useState(false);
  // #386: how the always-mounted New Run modal should open. Drives a one-shot
  // reset on every reopen so a dismissed "Edit trigger" can't leak into a fresh
  // "New run" / "New trigger". Defaults to a plain run.
  const [openIntent, setOpenIntent] = useState<OpenIntent>(RUN_INTENT);
  // #341: manual "Run now" — a refused fire (409: disabled / broken reference)
  // surfaces here as a dismissible banner.
  const [runNowError, setRunNowError] = useState<string | null>(null);
  // #341: bumped on every `trigger_fired` WS message so an open TriggerDetail
  // panel refetches its fire history (it otherwise only fetches on trigger.id).
  const [firesRefreshKey, setFiresRefreshKey] = useState(0);
  const [infoPanelOpen, setInfoPanelOpen] = useState(false);
  const [infoPanelInitialTab, setInfoPanelInitialTab] = useState<TabId | undefined>(undefined);
  const [infoPanelScrollToLine, setInfoPanelScrollToLine] = useState<number | undefined>(undefined);
  const mountedRef = useRef(false);
  const reloadPipeline = useEditStore((s) => s.reloadPipeline);
  const loadPipelines = useEditStore((s) => s.loadPipelines);
  const openRunPipeline = useEditStore((s) => s.openRunPipeline);
  const openPipeline = useEditStore((s) => s.openPipeline);
  const selection = useEditStore((s) => s.selection);
  const setSelection = useEditStore((s) => s.setSelection);
  const openTabs = useEditStore((s) => s.openTabs);
  const editSave = useEditStore((s) => s.save);
  const editUndo = useEditStore((s) => s.undo);
  const editRedo = useEditStore((s) => s.redo);
  const editActiveTabId = useEditStore((s) => s.activeTabId);
  const resolveConflict = useEditStore((s) => s.resolveConflict);
  const clearSaveError = useEditStore((s) => s.clearSaveError);
  // #342: a single-tab open/replace parked because it would discard unsaved
  // work — resolved by the global confirm modal below.
  const pendingSingleTab = useEditStore((s) => s.pendingSingleTab);
  const confirmPendingSingleTab = useEditStore((s) => s.confirmPendingSingleTab);
  const cancelPendingSingleTab = useEditStore((s) => s.cancelPendingSingleTab);
  // Merged /pipelines list — used to derive the selected trigger's prompt-required
  // signal for the guard dry-run caveat (#351).
  const pipelines = useEditStore((s) => s.pipelines);

  const editTab = openTabs.find((t) => t.id === editActiveTabId);
  const editNode = editTab && selection.kind === "node" && selection.id
    ? editTab.pipeline.nodes.find((n) => n.id === selection.id) ?? null
    : null;
  const editNodeType = editNode?.type ?? null;

  // Runtime trigger status for the selected edge (#147). Derived from the run
  // state when editing a run; the canvas never renders it.
  const selectedEdge =
    editTab && selection.kind === "edge" && selection.edgeIndex != null
      ? editTab.pipeline.edges[selection.edgeIndex] ?? null
      : null;
  const edgeTrigger =
    selectedEdge && editTab?.scope === "run"
      ? deriveEdgeTrigger(selectedRun, selectedEdge)
      : null;

  const isEditingRun = editTab?.scope === "run";
  const hasEditTab = editTab != null;
  // #684: which pane a selected node gets. Markers (start/end) never reach the
  // generic `NodeInspector` — outside a run they get a read-only pane.
  const nodeInspectorKind = resolveNodeInspector({
    nodeType: editNodeType,
    isEditingRun,
    hasRunStart: selectedRun?.start_node != null,
    hasRunEnd: selectedRun?.end_node != null,
  });

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

  // The Trigger backing the right-panel detail view (#162), the pane-owner
  // precedence (#247), the two render-time reconciliations (#320 canvas-reclaim,
  // #385 info auto-close), and the synthesized pending run-pane node all live in
  // useRightPaneRouter (#359). It runs the setState-during-render reconciliations
  // in-body here (App owns `selectedTriggerId`/`infoPanelOpen`, passed in with
  // their setters) so no stale frame of a shadowed panel is painted.
  const { paneOwner, selectedTrigger, triggerPromptRequired, runNode } =
    useRightPaneRouter({
      selection,
      editActiveTabId,
      hasEditTab,
      selectedTriggerId,
      setSelectedTriggerId,
      triggerOpenedTabId,
      infoPanelOpen,
      setInfoPanelOpen,
      triggers,
      pipelines,
      selectedRun,
    });

  const openNewRunModal = useCallback((intent: OpenIntent) => {
    setOpenIntent(intent);
    setNewRunModalOpen(true);
  }, []);

  const handleSelectTrigger = useCallback(
    (triggerId: string) => {
      // Selecting a Trigger clears the run/node selection so the detail panel
      // wins the right pane.
      setSelectedTriggerId(triggerId);
      setSelectedRunId(null);
      setSelectedNodeId(null);
      selectRun(null);
      // #320: also open the instance-owned pipeline this Trigger would launch.
      const trig = triggers.find((t) => t.id === triggerId);
      if (trig) {
        setTriggerOpenedTabId(trig.pipeline_id);
        openPipeline(trig.pipeline_id);
      }
    },
    [selectRun, triggers, openPipeline],
  );

  // #341: "Run now" is a real fire (guard + overlap + audit row), not a
  // prefilled New Run modal. Fire, then open the trigger detail (via
  // handleSelectTrigger — the only path that survives the #320 reconciliation)
  // where the new history row appears. A 409 (disabled / broken reference)
  // surfaces as a banner.
  const handleRunNowTrigger = useCallback(
    async (t: Trigger) => {
      // #348 (D3-A permissive): a manual fire still works while triggers are
      // globally paused — but behind a confirmation, so it stays a deliberate
      // escape hatch rather than a silent bypass of the kill-switch.
      if (
        triggersPaused &&
        !window.confirm(`Triggers are globally paused. Fire "${t.name}" anyway?`)
      ) {
        return;
      }
      try {
        const { fireTrigger } = await import("./api");
        await fireTrigger(t.id);
        setRunNowError(null);
      } catch (e) {
        setRunNowError(e instanceof Error ? e.message : String(e));
      }
      handleSelectTrigger(t.id);
    },
    [handleSelectTrigger, triggersPaused],
  );

  // #348: flip the global kill-switch. Optimistic (the master switch reflects the
  // intent immediately); the daemon's WS broadcast re-affirms it for every client,
  // and a failure reverts so a dead switch can't mislead the operator.
  const handleTogglePause = useCallback(async () => {
    const next = !triggersPaused;
    setTriggersPaused(next);
    try {
      await pauseTriggers(next);
    } catch {
      setTriggersPaused(!next);
    }
  }, [triggersPaused]);

  const handleCloseNewRunModal = useCallback(() => {
    setNewRunModalOpen(false);
    // Reset the intent to a plain run. Harmless while closed (the modal's reset
    // only fires on open false→true), but keeps the next default-less open clean.
    setOpenIntent(RUN_INTENT);
  }, []);

  const { activeTab: inspectorTab, setActiveTab: setInspectorTab } =
    useInspectorTab(editActiveTabId, isEditingRun);
  const nodeInspectorProvisioningProps = {
    provisioningRepository: isEditingRun ? selectedRun?.target_repo ?? "" : "",
    provisioningFrozenAt:
      isEditingRun && selection.id
        ? selectedRun?.nodes[selection.id]?.provisioning_frozen_at ?? undefined
        : undefined,
    inheritedProvisioning: isEditingRun
      ? selectedRun?.provisioning_rules
      : undefined,
    provisioningGitRef:
      isEditingRun && selectedRun ? `pdo/run-${selectedRun.run_id}` : "HEAD",
    runNode:
      isEditingRun && selection.id ? selectedRun?.nodes?.[selection.id] : null,
    // #669: the Run tier's frozen skills, shown as inherited in the inspector.
    runSkills: isEditingRun ? selectedRun?.skills ?? undefined : undefined,
  };

  // Both inspector panes are always rendered (with the inactive one hidden
  // via the `hidden` attribute) so that switching tabs does not unmount the
  // Run pane's `<NodeDetailPanel>` — which would tear the tmux WebSocket
  // down and reattach, pushing terminal content upward on every flip.
  function inspectorRunPane() {
    if (isEditingRun && selectedRun && runNode) {
      return (
        <NodeDetailPanel
          key={runNode.node_id}
          node={runNode}
          runId={selectedRun.run_id}
          isArchived={isArchived}
          nodeName={selectedRun.node_defs?.find((d) => d.id === selection.id)?.name}
          provisioningRepository={selectedRun.target_repo ?? ""}
          inheritedProvisioning={selectedRun.provisioning_rules}
          provisioningGitRef={`pdo/run-${selectedRun.run_id}`}
        />
      );
    }
    if (isEditingRun && selectedRun) {
      return <RunTabPlaceholder nodeId={selection.id} />;
    }
    return <NoRunPlaceholder />;
  }

  function inspectorEditPane() {
    switch (editNodeType) {
      case "merge": return <MergeInspector />;
      // #248: `script` reuses NodeInspector, which shows the Script (bash) editor
      // and hides the model field for it.
      // Without this case a script node would fall through and — before the
      // in-inspector conditionals — render the wrong (agent) surface.
      case "script":
      default: return (
        <NodeInspector
          libraryEntries={libraryEntries}
          onLibraryChanged={refreshLibrary}
          readOnly={isActiveRunArchived}
          {...nodeInspectorProvisioningProps}
        />
      );
    }
  }

  const handleToggleInfo = useCallback(() => {
    setInfoPanelOpen((prev) => {
      if (!prev) {
        setInfoPanelInitialTab(undefined);
        setInfoPanelScrollToLine(undefined);
      }
      return !prev;
    });
  }, []);

  const handleCloseInfo = useCallback(() => {
    setInfoPanelOpen(false);
  }, []);

  // #302 / ADR-0048: the toolbar Bot glyph opens the info panel focused on the
  // Assistant tab (the library authoring copilot). Same shape as `handleViewYaml`:
  // set the initial tab, then open. The panel is keyed on `infoPanelInitialTab`,
  // so this remounts it at the Assistant tab.
  const handleOpenAssistant = useCallback(() => {
    setInfoPanelInitialTab("assistant");
    setInfoPanelScrollToLine(undefined);
    setInfoPanelOpen(true);
  }, []);

  useEffect(() => {
    if (!mountedRef.current) {
      mountedRef.current = true;
      refreshRuns();
      refreshSessions();
      refreshTriggers();
      refreshProjects();
      // #348: hydrate the global pause flag once — no trigger polling carries it,
      // so without this a page reload would show the banner off while paused.
      fetchTriggersHealth()
        .then((h) => setTriggersPaused(h.paused))
        .catch(() => {});
      useRecentReposStore.getState().refresh();
    }
  }, [refreshRuns, refreshSessions, refreshTriggers, refreshProjects]);

  // A WS reconnect usually means the daemon restarted — possibly as a different
  // binary, so the /sessions payload (version included, #139) may be stale. An
  // idle daemon emits no event afterwards, so the subscribe-side refresh never
  // fires; re-fetch on every transition to "connected".
  useEffect(() => {
    if (status === "connected") {
      refreshSessions();
    }
  }, [status, refreshSessions]);

  // On a live run with nothing selected, snap selection to the latest
  // running (or awaiting_user) node so the user immediately sees its terminal.
  // Re-fires whenever the user deselects on a still-live run — but never over a
  // deliberate selection (a node, an inspector, or the Run-info sidebar, #465
  // slice 2, F1). The gate is `shouldAutoSnapToLiveNode`; this effect only
  // resolves the concrete node and applies it.
  useEffect(() => {
    if (!selectedRun) return;
    if (!shouldAutoSnapToLiveNode(selection.kind, !!selection.id, selectedRun.status)) return;
    const nodeId = pickLatestLiveNode(selectedRun);
    if (!nodeId) return;
    setSelection({ kind: "node", id: nodeId });
  }, [selectedRun, selection.kind, selection.id, setSelection]);

  const handleSelectRun = useCallback(
    async (runId: string) => {
      setSelectedTriggerId(null);
      setSelectedRunId(runId);
      selectRun(runId);
      setSelectedNodeId(null);
      await openRunPipeline(runId);
    },
    [selectRun, openRunPipeline],
  );

  const handleRunCreated = useCallback(
    (runId: string) => {
      refreshRuns();
      handleSelectRun(runId);
    },
    [refreshRuns, handleSelectRun],
  );

  useEffect(() => {
    // #315: never fire a save for an archived run — the tab is read-only and a
    // PUT would 404. `isActiveRunArchived` also removes this listener the moment
    // the open run flips to archived (via refreshRun).
    if (!hasEditTab || isActiveRunArchived) return;
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "s") {
        e.preventDefault();
        if (editActiveTabId) editSave(editActiveTabId);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [hasEditTab, isActiveRunArchived, editActiveTabId, editSave]);

  // Canvas undo/redo (ADR-0014 / #226): Ctrl/Cmd+Z undo, Ctrl/Cmd+Shift+Z or
  // Ctrl/Cmd+Y redo. Sibling to the Ctrl+S effect above, but — unlike Save — it
  // MUST yield to native field undo while a text field is focused. The branch
  // logic (and that input-focus guard) lives in `handleUndoRedoKeydown` so it's
  // unit-testable without rendering the canvas; this effect just wires it up.
  useEffect(() => {
    // #315: undo/redo are edit affordances — off for a read-only archived run.
    if (!hasEditTab || isActiveRunArchived) return;
    const handler = (e: KeyboardEvent) => handleUndoRedoKeydown(e, editUndo, editRedo);
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [hasEditTab, isActiveRunArchived, editUndo, editRedo]);

  // #368: keep the ref in step with the selected Trigger so the stable WS
  // callback below reads the committed value, not the stale-null closure.
  useEffect(() => {
    selectedTriggerIdRef.current = selectedTriggerId;
  }, [selectedTriggerId]);

  useEffect(() => {
    return subscribe((msg) => {
      if (msg.type === "pipeline_changed" && msg.pipeline_id) {
        reloadPipeline(msg.pipeline_id);
        loadPipelines();
        return;
      }
      // #348: global pause flip — reflect it live across every client. The
      // per-Trigger `enabled` state (and hence the Triggers list) is untouched,
      // so no refresh is needed; just mirror the flag.
      if (msg.type === "triggers_paused") {
        setTriggersPaused(!!msg.paused);
        return;
      }
      // #552: a Projet mutation (name/harness/members) — refresh the Projet list
      // so the Runs and Triggers regrouping is live across clients. The Runs /
      // Triggers rows themselves are unchanged, so only the Projet list refetches.
      if (msg.type === "project_changed") {
        refreshProjects();
        return;
      }
      // Trigger lifecycle (#160/#162): create/update/delete refreshes the
      // Triggers list; a fire also creates a Run, so refresh both.
      if (
        msg.type === "trigger_created" ||
        msg.type === "trigger_updated" ||
        msg.type === "trigger_deleted" ||
        msg.type === "trigger_fired"
      ) {
        refreshTriggers();
        if (msg.type === "trigger_fired") {
          refreshRuns();
          // #341 + #368: only the OPEN trigger's panel refetches, so an
          // unrelated fire doesn't flash/reload the currently-viewed history.
          // Read the ref (not selectedTriggerId) to keep this stable callback
          // out of the effect deps and dodge the stale-null closure.
          if (msg.trigger_id === selectedTriggerIdRef.current) {
            setFiresRefreshKey((v) => v + 1);
          }
        }
        return;
      }
      // #315: an archived run's outputs are now preserved (ADR-0020) and its
      // `/pipeline` endpoint keeps serving, so we no longer prune the open tab
      // on `run_archived`. `refreshRun` below flips the run's status to
      // `archived`, which turns the open canvas read-only in place — the run the
      // user is watching stays put instead of vanishing.
      refreshRuns();
      refreshRun();
      // Node start/complete/fail/waiting transitions change the live session
      // count (#159) — keep the status-bar counter current.
      refreshSessions();
    });
  }, [subscribe, refreshRuns, refreshRun, refreshSessions, refreshTriggers, refreshProjects, reloadPipeline, loadPipelines]);

  const selectedNode =
    selectedNodeId && selectedRun
      ? selectedRun.nodes[selectedNodeId] ?? null
      : null;

  const selectedNodeType = selectedRun?.node_defs?.find(
    (d) => d.id === selectedNodeId,
  )?.node_type ?? null;

  const layout = useResizableLayout("run", PANEL_IDS, DEFAULT_SIZES);
  const minSizePx = `${layout.minSizePx}px`;
  const conflictTab = openTabs.find((t) => t.conflict != null);
  const saveErrorTab = openTabs.find((t) => t.saveError != null);

  const handleDismissSaveError = useCallback(() => {
    if (saveErrorTab) clearSaveError(saveErrorTab.id);
  }, [saveErrorTab, clearSaveError]);

  const handleViewYaml = useCallback(() => {
    if (!saveErrorTab) return;
    setInfoPanelInitialTab("yaml");
    setInfoPanelScrollToLine(saveErrorTab.saveError?.line);
    setInfoPanelOpen(true);
    clearSaveError(saveErrorTab.id);
  }, [saveErrorTab, clearSaveError]);

  return (
    <TooltipProvider>
    <div className="flex h-full flex-col bg-bg-1 text-fg">
      <TopBar
        onOpenSettings={() => setSettingsModalOpen(true)}
        onOpenStats={() => setStatsModalOpen(true)}
      />
      <main className="min-h-0 flex-1">
        <ResizablePanelGroup
          orientation="horizontal"
          defaultLayout={layout.defaultLayout}
          onLayoutChanged={layout.onLayoutChanged}
        >
          <ResizablePanel defaultSize={layout.defaultLayout.left} minSize={minSizePx} id="left">
            <UnifiedLeftPanel
              runs={runs}
              selectedRunId={selectedRunId}
              onSelectRun={handleSelectRun}
              onNewRun={() => openNewRunModal({ kind: "run" })}
              triggers={triggers}
              selectedTriggerId={selectedTriggerId}
              onSelectTrigger={handleSelectTrigger}
              onNewTrigger={() => openNewRunModal({ kind: "new-trigger" })}
              onTriggersChanged={refreshTriggers}
              onRunNowTrigger={handleRunNowTrigger}
              onEditTrigger={(t) => openNewRunModal({ kind: "edit-trigger", trigger: t })}
              triggersPaused={triggersPaused}
              onTogglePause={handleTogglePause}
              projects={projects}
              onProjectsChanged={refreshProjects}
            />
          </ResizablePanel>

          <ResizableHandle />

          <ResizablePanel defaultSize={layout.defaultLayout.center} id="center">
            {hasEditTab ? (
              <div className="flex h-full min-w-0 flex-col">
                <TabBar />
                <EditCanvas
                  libraryEntries={libraryEntries}
                  onLibraryDelete={async (name) => {
                    const { deleteFromLibrary: delLib } = await import("./api");
                    await delLib(name);
                    refreshLibrary();
                  }}
                  infoOpen={infoPanelOpen}
                  onToggleInfo={handleToggleInfo}
                  onCloseInfo={handleCloseInfo}
                  assistantActive={infoPanelOpen && infoPanelInitialTab === "assistant"}
                  onOpenAssistant={handleOpenAssistant}
                  runState={selectedRun}
                  onSelectRun={handleSelectRun}
                />
              </div>
            ) : (
              <div className="flex h-full items-center justify-center text-fg-4" style={{ fontSize: "12px" }}>
                Select a run or open a pipeline to get started
              </div>
            )}
          </ResizablePanel>

          <ResizableHandle />

          <ResizablePanel defaultSize={layout.defaultLayout.right} minSize={minSizePx} id="right" className="panel-r">
            {paneOwner === "trigger" && selectedTrigger ? (
              <TriggerDetailPanel
                key={selectedTrigger.id}
                trigger={selectedTrigger}
                onSelectRun={handleSelectRun}
                refreshKey={firesRefreshKey}
                promptRequired={triggerPromptRequired}
              />
            ) : paneOwner === "info" ? (
              <PipelineInfoPanel
                key={infoPanelInitialTab ?? "default"}
                run={isEditingRun ? selectedRun : null}
                pipeline={editTab?.pipeline ?? null}
                onClose={handleCloseInfo}
                initialTab={infoPanelInitialTab}
                scrollToLine={infoPanelScrollToLine}
                assistantId={assistantId}
              />
            ) : paneOwner === "editTab" ? (
              <>
                {selection.kind === "node" && editNodeType != null && nodeInspectorKind === "node" ? (
                  <InspectorTabs activeTab={inspectorTab} onTabChange={setInspectorTab}>
                    <div hidden={inspectorTab !== "run"} className="h-full" data-testid="inspector-pane-run">
                      {inspectorRunPane()}
                    </div>
                    <div hidden={inspectorTab !== "edit"} className="h-full" data-testid="inspector-pane-edit">
                      {inspectorEditPane()}
                    </div>
                  </InspectorTabs>
                ) : selection.kind === "node" && nodeInspectorKind === "run-start" && selectedRun?.start_node && selection.id ? (
                  <StartInspector
                    startNode={selectedRun.start_node}
                    runId={selectedRun.run_id}
                    nodeId={selection.id}
                  />
                ) : selection.kind === "node" && nodeInspectorKind === "run-end" && selectedRun?.end_node ? (
                  <EndInspector
                    endNode={selectedRun.end_node}
                  />
                ) : selection.kind === "node" && nodeInspectorKind === "marker" && editNode ? (
                  <MarkerInspector key={editNode.id} node={editNode} />
                ) : selection.kind === "node" ? (
                  <NodeInspector
                    libraryEntries={libraryEntries}
                    onLibraryChanged={refreshLibrary}
                    readOnly={isActiveRunArchived}
                    {...nodeInspectorProvisioningProps}
                  />
                ) : selection.kind === "edge" ? (
                  <EdgeDetailPanel trigger={edgeTrigger} />
                ) : selection.kind === "region" ? (
                  <RegionInspector />
                ) : selection.kind === "note" ? (
                  <NoteInspector />
                ) : null}
                {/* `"none"` reaches this on a terminal/paused run (deselect, or
                    selecting the run — #503 red-dot panel); `"run"` is the
                    explicit toggle that keeps it reachable while a live node
                    runs (#465 slice 2, F1). */}
                {(selection.kind === "none" || selection.kind === "run") &&
                  isEditingRun &&
                  selectedRun && (
                    <RunInfoSidebar run={selectedRun} onEdited={refreshRun} />
                  )}
                {selection.kind === "none" && !isEditingRun && (
                  <PipelineInspector />
                )}
              </>
            ) : (
              <>
                {selectedNodeType === "start" && selectedRun?.start_node && (
                  <StartInspector
                    startNode={selectedRun.start_node}
                    runId={selectedRun.run_id}
                    nodeId={selectedNodeId!}
                  />
                )}
                {selectedNodeType === "end" && selectedRun?.end_node && (
                  <EndInspector
                    endNode={selectedRun.end_node}
                  />
                )}
                {selectedNode && selectedRun && selectedNodeType !== "start" && selectedNodeType !== "end" && (
                  <NodeDetailPanel
                    key={selectedNode.node_id}
                    node={selectedNode}
                    runId={selectedRun.run_id}
                    isArchived={isArchived}
                    nodeName={selectedRun.node_defs?.find((d) => d.id === selectedNodeId)?.name}
                  />
                )}
                {!selectedNode && selectedNodeType !== "start" && isArchived && selectedRun && (
                  <aside className="flex h-full flex-col items-center justify-center bg-bg-2 text-fg-4" style={{ fontSize: "12px" }}>
                    <div className="text-center px-6">
                      <div className="font-medium text-fg-3">Run archived</div>
                      <div className="mt-1">No live state available. Select a node to view its final status.</div>
                    </div>
                  </aside>
                )}
              </>
            )}
          </ResizablePanel>
        </ResizablePanelGroup>
      </main>
      <StatusBar status={status} sessions={sessions} />
      <NewRunModal
        open={newRunModalOpen}
        onClose={handleCloseNewRunModal}
        onCreated={(runId) => {
          handleCloseNewRunModal();
          handleRunCreated(runId);
        }}
        openIntent={openIntent}
        onTriggerSaved={refreshTriggers}
      />
      <SettingsModal
        open={settingsModalOpen}
        onClose={() => setSettingsModalOpen(false)}
        liveSessions={sessions.live}
        onSaved={refreshSessions}
      />
      <StatsModal open={statsModalOpen} onClose={() => setStatsModalOpen(false)} />
      <ConflictModal
        open={conflictTab != null}
        pipelineId={conflictTab?.id ?? ""}
        onKeep={() => {
          if (conflictTab) resolveConflict(conflictTab.id, "keep");
        }}
        onTake={() => {
          if (conflictTab) resolveConflict(conflictTab.id, "take");
        }}
      />
      <SaveErrorModal
        open={saveErrorTab != null}
        error={saveErrorTab?.saveError ?? null}
        onDismiss={handleDismissSaveError}
        onViewYaml={handleViewYaml}
      />
      {/* #342: single-tab open/replace (and enable-collapse) that would discard
          unsaved work parks here for a global confirmation. */}
      <ConfirmCloseTabsModal
        open={pendingSingleTab != null}
        tabs={pendingSingleTab?.victims ?? []}
        onCancel={cancelPendingSingleTab}
        onConfirm={confirmPendingSingleTab}
      />
      {runNowError && (
        <div
          className="fixed bottom-3 left-1/2 z-50 flex -translate-x-1/2 items-center gap-2 rounded border border-st-failed bg-bg-2 px-3 py-2 text-fg"
          style={{ fontSize: "11.5px" }}
          data-testid="run-now-error"
        >
          <span>{runNowError}</span>
          <button
            onClick={() => setRunNowError(null)}
            className="text-fg-3 hover:text-fg"
            data-testid="run-now-error-dismiss"
          >
            ✕
          </button>
        </div>
      )}
    </div>
    </TooltipProvider>
  );
}

function TopBar({
  onOpenSettings,
  onOpenStats,
}: {
  onOpenSettings: () => void;
  onOpenStats: () => void;
}) {
  return (
    <header
      className="flex h-[44px] shrink-0 items-center gap-3 border-b border-line bg-bg-2 px-3"
      style={{ fontSize: "12.5px" }}
    >
      <div className="flex items-center gap-2 border-r border-line pr-3 font-semibold tracking-tight text-fg">
        <span className="grid h-[18px] w-[18px] place-items-center text-acc">
          <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
            <path
              d="M7 1L12.5 4.5V9.5L7 13L1.5 9.5V4.5L7 1Z"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinejoin="round"
            />
            <circle cx="7" cy="7" r="2" fill="currentColor" />
          </svg>
        </span>
        PDO
      </div>

      {/* Right-aligned action group: stats (#377) then the settings gear (#129). */}
      <div className="ml-auto flex items-center gap-1">
        <button
          onClick={onOpenStats}
          aria-label="Open stats"
          data-testid="open-stats"
          className="grid h-6 w-6 place-items-center rounded text-fg-3 transition-colors hover:bg-bg-5 hover:text-fg"
        >
          <BarChart3 size={15} />
        </button>
        <button
          onClick={onOpenSettings}
          aria-label="Settings"
          data-testid="open-settings"
          className="grid h-6 w-6 place-items-center rounded text-fg-3 transition-colors hover:bg-bg-5 hover:text-fg"
        >
          <Settings size={15} />
        </button>
      </div>
    </header>
  );
}

function RunTabPlaceholder({ nodeId }: { nodeId: string | null }) {
  return (
    <aside
      className="flex h-full flex-col items-center justify-center bg-bg-2 text-fg-4"
      style={{ fontSize: "12px" }}
      data-testid="pending-placeholder"
    >
      <div className="px-6 text-center">
        <div className="font-medium text-fg-3">
          <em>en attente d&apos;activation</em>
        </div>
        {nodeId && (
          <div className="mt-1 font-mono" style={{ fontSize: "10px" }}>
            {nodeId}
          </div>
        )}
        <div className="mt-2 text-fg-4" style={{ fontSize: "11px" }}>
          This node is waiting for upstream dependencies to complete.
        </div>
      </div>
    </aside>
  );
}

function NoRunPlaceholder() {
  return (
    <aside
      className="flex h-full flex-col items-center justify-center bg-bg-2 text-fg-4"
      style={{ fontSize: "12px" }}
    >
      <div className="px-6 text-center">
        <div className="font-medium text-fg-3">No active run</div>
        <div className="mt-1">
          Launch a run to see execution state in this tab.
        </div>
      </div>
    </aside>
  );
}

const STATUS_CONFIG: Record<ConnectionStatus, { dot: string; label: string }> = {
  connected: { dot: "bg-st-done", label: "Daemon: connected" },
  reconnecting: { dot: "bg-st-await", label: "Daemon: reconnecting…" },
  disconnected: { dot: "bg-st-failed", label: "Daemon: disconnected" },
};

function StatusBar({
  status,
  sessions,
}: {
  status: ConnectionStatus;
  sessions: DaemonStatus;
}) {
  const { dot: dotClass, label } = STATUS_CONFIG[status];

  return (
    <footer
      className="flex h-[22px] shrink-0 items-center gap-3.5 border-t border-line bg-bg-2 px-3 font-mono text-fg-3"
      style={{ fontSize: "11px" }}
    >
      <span className="flex items-center gap-1.5">
        <span className={`h-1.5 w-1.5 rounded-full ${dotClass}`} />
        {label}
      </span>
      <span className="flex-1" />
      <ServiceHealthIndicator service={sessions.service} />
      <SessionCounter live={sessions.live} cap={sessions.cap} />
      {sessions.version && <span>v{sessions.version}</span>}
    </footer>
  );
}
