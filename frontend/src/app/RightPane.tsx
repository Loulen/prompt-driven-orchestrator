import { useCallback } from "react";
import { Archive } from "lucide-react";
import { EmptyState } from "../shared/ui/kit";
import type { LibraryEntry } from "../pipeline/model/library";
import type { RunState } from "../shared/generated/RunState";
import type { Trigger } from "../shared/generated/Trigger";
import type { NodeState } from "../shared/generated/NodeState";
import { isTabLocked, type OpenPipeline } from "../pipeline/store/editState";
import type { TabId } from "../lib/infoPanelReconcile";
import type { SettingsPosition } from "../settings/components/SettingsSurface";
import type { useRightPaneRouter } from "../hooks/useRightPaneRouter";
import NodeDetailPanel from "../run/components/NodeDetailPanel";
import NodeInspector from "../pipeline/components/NodeInspector";
import NodeConfigView from "../pipeline/components/NodeConfigView";
import PipelineInfoPanel from "../pipeline/components/PipelineInfoPanel";
import StartInspector from "../pipeline/components/StartInspector";
import EndInspector from "../pipeline/components/EndInspector";
import MarkerInspector from "../pipeline/components/MarkerInspector";
import EdgeDetailPanel from "../pipeline/components/EdgeDetailPanel";
import RegionInspector from "../pipeline/components/RegionInspector";
import NoteInspector from "../pipeline/components/NoteInspector";
import InspectorTabs from "../pipeline/components/InspectorTabs";
import TriggerDetailPanel from "../trigger/components/TriggerDetailPanel";
import type { Project } from "../workspace/model/project";
import { resolveNodeInspector } from "../lib/structuralMarkers";
import { deriveEdgeTrigger } from "../lib/edgeTrigger";
import { useInspectorTab } from "../hooks/useInspectorTab";
import { useOnHitChosen } from "../run/store/runSearchStore";
import { useEditStore } from "../pipeline/store/editStore";
import { RunTabPlaceholder } from "./RunPanePlaceholders";
import GuidePanel, { type GuideNavigation } from "../platform/components/GuidePanel";
import type { PageMount } from "../shared/generated/PageMount";

/** What the right pane shows, and what it hands back. */
export interface RightPaneProps {
  paneOwner: ReturnType<typeof useRightPaneRouter>["paneOwner"];
  selectedTrigger: Trigger | null | undefined;
  triggerPromptRequired: boolean;
  firesRefreshKey: number;
  /** The selected Trigger's « Run now » and « Edit » (its detail's header). */
  onRunNowTrigger?: (trigger: Trigger) => void;
  onEditTrigger?: (trigger: Trigger) => void;
  /** The Projets, to name the selected Trigger's. */
  projects?: Project[];
  runNode: NodeState | null | undefined;
  editTab: OpenPipeline | undefined;
  selectedRun: RunState | null;
  selectedNodeId: string | null;
  isArchived: boolean;
  isActiveRunArchived: boolean;
  assistantId: string | null;
  infoPanelTab: TabId;
  onInfoPanelTabChange: (tab: TabId) => void;
  infoPanelScrollToLine: number | undefined;
  onCloseInfo: () => void;
  libraryEntries: LibraryEntry[];
  onLibraryChanged: () => void;
  landOnOrchestration: string | null;
  onSelectRun: (runId: string) => void;
  onOpenChildRun: (childRunId: string) => void;
  onRefreshRun: () => void;
  openSettings: (position?: SettingsPosition) => void;
  /** The mounted page shown in the centre, whose guide owns the pane. */
  activePage?: PageMount | null;
  guideNavigation?: GuideNavigation;
}

/**
 * The right pane of the shell: a Trigger's detail, the pipeline info panel, the
 * inspector of what is selected on the canvas (with its Run and Edit panes), or
 * a Run's node detail — whichever `useRightPaneRouter` says owns it.
 */
export default function RightPane({
  paneOwner,
  selectedTrigger,
  triggerPromptRequired,
  firesRefreshKey,
  onRunNowTrigger,
  onEditTrigger,
  projects,
  runNode,
  editTab,
  selectedRun,
  selectedNodeId,
  isArchived,
  isActiveRunArchived,
  assistantId,
  infoPanelTab,
  onInfoPanelTabChange,
  infoPanelScrollToLine,
  onCloseInfo: handleCloseInfo,
  libraryEntries,
  onLibraryChanged: refreshLibrary,
  landOnOrchestration,
  onSelectRun: handleSelectRun,
  onOpenChildRun: handleOpenChildRun,
  onRefreshRun: refreshRun,
  openSettings,
  activePage,
  guideNavigation,
}: RightPaneProps) {
  const selection = useEditStore((s) => s.selection);
  const editActiveTabId = useEditStore((s) => s.activeTabId);
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
  // ADR-0085: a Run tab in « pilotage » (or archived): the inspector reads,
  // never writes — Config instead of Edit. ADR-0088: so does a pipeline tab
  // that only reads its shared draft.
  const runLocked = isTabLocked(editTab);
  // #684: which pane a selected node gets. Markers (start/end) never reach the
  // generic `NodeInspector` — outside a run they get a read-only pane.
  const nodeInspectorKind = resolveNodeInspector({
    nodeType: editNodeType,
    isEditingRun,
    hasRunStart: selectedRun?.start_node != null,
    hasRunEnd: selectedRun?.end_node != null,
  });

  const { activeTab: inspectorTab, setActiveTab: setInspectorTab } =
    useInspectorTab(editActiveTabId, isEditingRun);
  // A hit chosen in the Run's search lands on the node's Run view.
  useOnHitChosen(useCallback(() => setInspectorTab("run"), [setInspectorTab]));
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
          runStatus={selectedRun.status}
          nodeName={selectedRun.node_defs?.find((d) => d.id === selection.id)?.name}
          provisioningRepository={selectedRun.target_repo ?? ""}
          inheritedProvisioning={selectedRun.provisioning_rules}
          provisioningGitRef={`pdo/run-${selectedRun.run_id}`}
          isOrchestratorNode={selectedRun.node_defs?.find((d) => d.id === selection.id)?.orchestrator ?? false}
          isOnDemand={editNode?.on_demand ?? false}
          onOpenChildRun={handleOpenChildRun}
          initialDetailTab={landOnOrchestration === runNode.node_id ? "orchestration" : "io"}
        />
      );
    }
    return <RunTabPlaceholder nodeId={selection.id} />;
  }

  function inspectorEditPane() {
    if (runLocked) {
      return (
        <NodeConfigView
          provisioningRepository={nodeInspectorProvisioningProps.provisioningRepository}
          runSkills={nodeInspectorProvisioningProps.runSkills}
          runNode={nodeInspectorProvisioningProps.runNode}
          archived={isActiveRunArchived}
        />
      );
    }
    switch (editNodeType) {
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

  const selectedNode =
    selectedNodeId && selectedRun
      ? selectedRun.nodes[selectedNodeId] ?? null
      : null;

  const selectedNodeType = selectedRun?.node_defs?.find(
    (d) => d.id === selectedNodeId,
  )?.node_type ?? null;

  return (
    <>
        {paneOwner === "page" ? (
          activePage?.guide?.length && guideNavigation ? (
            <GuidePanel key={activePage.name} page={activePage} navigation={guideNavigation} />
          ) : null
        ) : paneOwner === "trigger" && selectedTrigger ? (
          <TriggerDetailPanel
            key={selectedTrigger.id}
            trigger={selectedTrigger}
            onSelectRun={handleSelectRun}
            refreshKey={firesRefreshKey}
            promptRequired={triggerPromptRequired}
            onRunNow={onRunNowTrigger ? () => onRunNowTrigger(selectedTrigger) : undefined}
            onEdit={onEditTrigger ? () => onEditTrigger(selectedTrigger) : undefined}
            projectName={projects?.find((p) => p.id === selectedTrigger.project_id)?.name ?? null}
          />
        ) : paneOwner === "info" ? (
          <PipelineInfoPanel
            run={isEditingRun ? selectedRun : null}
            pipeline={editTab?.pipeline ?? null}
            onClose={handleCloseInfo}
            tab={infoPanelTab}
            onTabChange={onInfoPanelTabChange}
            scrollToLine={infoPanelScrollToLine}
            assistantId={assistantId}
            onRefreshRun={refreshRun}
            onOpenSettings={() => openSettings({ category: "agents", section: "pipeline-manager" })}
          />
        ) : paneOwner === "editTab" ? (
          <>
            {selection.kind === "node" && editNodeType != null && nodeInspectorKind === "node" && !isEditingRun ? (
              // CORE-88: outside a Run there is nothing to show in a « Run » tab —
              // the edition opens directly, without the tab bar.
              <div className="h-full" data-testid="inspector-pane-edit">
                {inspectorEditPane()}
              </div>
            ) : selection.kind === "node" && editNodeType != null && nodeInspectorKind === "node" ? (
              <InspectorTabs activeTab={inspectorTab} onTabChange={setInspectorTab} configMode={runLocked}>
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
                readOnly={isActiveRunArchived || runLocked}
                {...nodeInspectorProvisioningProps}
              />
            ) : selection.kind === "edge" || selection.kind === "region" || selection.kind === "note" ? (
              // ADR-0085: in « pilotage » these panes are read, not edited: a
              // disabled fieldset turns every control off at once (the store
              // refuses the mutation anyway).
              <fieldset disabled={runLocked} className="contents" data-testid="inspector-locked" data-locked={runLocked}>
                {selection.kind === "edge" ? (
                  <EdgeDetailPanel trigger={edgeTrigger} />
                ) : selection.kind === "region" ? (
                  <RegionInspector />
                ) : (
                  <NoteInspector />
                )}
              </fieldset>
            ) : null}
            {/* `"none"` reaches this on a terminal/paused run (deselect, or
                selecting the run — #503 red-dot panel). #752: the standalone
                Run-info sidebar is gone; the Run panel opens on Info, which
                now carries the failure / awaiting reason, and Repositories
                is one of its tabs (`Info | Diff | Repositories | Manager | YAML`). */}
            {(selection.kind === "none" || selection.kind === "run") &&
              isEditingRun &&
              selectedRun && (
                <PipelineInfoPanel
                  key={`run-panel-${selectedRun.run_id}`}
                  run={selectedRun}
                  pipeline={editTab?.pipeline ?? null}
                  onClose={handleCloseInfo}
                  initialTab="info"
                  onRefreshRun={refreshRun}
                  onOpenSettings={() => openSettings({ category: "agents", section: "pipeline-manager" })}
                />
              )}
            {/* Notion #6 / #948: an empty selection on a template shows
                nothing — its settings live in Pipeline info, opened by `i`;
                the pane collapses (#949). */}
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
                runStatus={selectedRun.status}
                nodeName={selectedRun.node_defs?.find((d) => d.id === selectedNodeId)?.name}
                isOrchestratorNode={selectedRun.node_defs?.find((d) => d.id === selectedNodeId)?.orchestrator ?? false}
                isOnDemand={selectedRun.node_defs?.find((d) => d.id === selectedNodeId)?.on_demand ?? false}
                onOpenChildRun={handleOpenChildRun}
                initialDetailTab={landOnOrchestration === selectedNode.node_id ? "orchestration" : "io"}
              />
            )}
            {!selectedNode && selectedNodeType !== "start" && isArchived && selectedRun && (
              <aside className="flex h-full flex-col items-center justify-center bg-surface-1">
                <EmptyState icon={Archive}>Run archived</EmptyState>
              </aside>
            )}
          </>
        )}
    </>
  );
}
