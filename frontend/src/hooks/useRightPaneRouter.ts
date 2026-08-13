import { useState } from "react";
import { rightPaneOwner } from "../lib/rightPaneOwner";
import type { RightPaneOwner } from "../lib/rightPaneOwner";
import { shouldClearTriggerOnCanvasFocus } from "../lib/triggerCanvasReconcile";
import { shouldCloseInfoOnTabChange } from "../lib/infoPanelReconcile";
import type { Selection } from "../stores/editStore";
import type {
  Trigger,
  PipelineListEntry,
  RunState,
  NodeState,
} from "../types";
import { isLiveRun } from "../types";

export interface RightPaneRouterArgs {
  /** The current canvas selection (`editStore.selection`). */
  selection: Selection;
  /** The active edit-tab id (`editStore.activeTabId`). */
  editActiveTabId: string | null;
  /** Whether an edit tab owns the centre canvas. */
  hasEditTab: boolean;
  /** The currently-selected Trigger id (App-owned state). */
  selectedTriggerId: string | null;
  /** Setter for `selectedTriggerId` — the #320 reconciliation clears it. */
  setSelectedTriggerId: (id: string | null) => void;
  /** The tab id that selecting a Trigger opened in the canvas (#320). */
  triggerOpenedTabId: string | null;
  /** Whether the Pipeline Info peek overlay is open (App-owned state). */
  infoPanelOpen: boolean;
  /** Setter for `infoPanelOpen` — the #385 reconciliation closes it. */
  setInfoPanelOpen: (open: boolean) => void;
  /** The Triggers list — used to resolve `selectedTrigger`. */
  triggers: Trigger[];
  /** The merged /pipelines list — used for the guard dry-run caveat (#351). */
  pipelines: PipelineListEntry[];
  /** The selected run — used to synthesize a pending run-pane node. */
  selectedRun: RunState | null;
}

export interface RightPaneRouterResult {
  /** Which view owns the right-hand detail pane (#247). */
  paneOwner: RightPaneOwner;
  /** The Trigger backing the right-panel detail view, or null. */
  selectedTrigger: Trigger | null;
  /** Whether the selected Trigger's pipeline requires a prompt (#351). */
  triggerPromptRequired: boolean;
  /** A synthesized pending NodeState for the Run pane, or null. */
  runNode: NodeState | null;
}

/**
 * The right-pane router (#359, extracted from App.tsx).
 *
 * Composes the two render-time reconciliations (#320 canvas-reclaim, #385 info
 * auto-close), the pane-owner precedence (#247, `rightPaneOwner`), and the
 * derived detail-view state (`selectedTrigger`, its prompt-required signal, and
 * the synthesized pending run-pane node) that together decide what the
 * right-hand pane shows.
 *
 * Both reconciliations run as setState-DURING-render (React's recommended
 * reset-on-change idiom, NOT `useEffect`) so no stale frame of the shadowed
 * panel is ever painted (#247/#385). This hook is called at the top of App's
 * render, so those setState calls happen during App's render exactly as before:
 * `selectedTriggerId`/`infoPanelOpen` are App-owned (passed in with their
 * setters); the two order-trackers (`lastCanvasFocus`, `lastInfoTabId`) are
 * owned here. LOAD-BEARING: reconcile BEFORE compute-owner, in the SAME render
 * pass; advance each tracker UNCONDITIONALLY (gating it would re-fire the block
 * every render — infinite loop); never move the clears into an effect.
 */
export function useRightPaneRouter(
  args: RightPaneRouterArgs,
): RightPaneRouterResult {
  const {
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
  } = args;

  // #320 canvas-reclaim. A Trigger detail and a canvas selection compete for the
  // right pane (#247). The Trigger wins over a *persistent* run-edit tab (see
  // rightPaneOwner), so the canvas needs an explicit way to reclaim the pane —
  // otherwise a once-selected Trigger would shadow every later node/edge/region
  // inspector. Selecting a Trigger touches neither `selection` nor the active
  // tab, so the canvas-focus signal below never fires on a fresh Trigger
  // selection; any later canvas focus (a node/edge/region selection, or a tab
  // switch/open) clears it. Adjusting state during render rather than in an
  // effect avoids painting one stale frame of the Trigger detail.
  const [lastCanvasFocus, setLastCanvasFocus] = useState({
    selection,
    tabId: editActiveTabId,
  });
  if (
    lastCanvasFocus.selection !== selection ||
    lastCanvasFocus.tabId !== editActiveTabId
  ) {
    // setLastCanvasFocus MUST stay unconditional — gating it would make this
    // block re-fire every render (infinite loop).
    setLastCanvasFocus({ selection, tabId: editActiveTabId });
    // #320: skip clearing the Trigger when this focus change is the Trigger's
    // OWN openPipeline landing — i.e. the tab it opened is now active with
    // nothing selected. A genuine canvas reclaim differs and STILL clears
    // (preserving #247): a node/edge/region select makes selection.kind !==
    // "none", and switching to another tab makes editActiveTabId !==
    // triggerOpenedTabId.
    if (
      shouldClearTriggerOnCanvasFocus({
        selectedTriggerId,
        editActiveTabId,
        selectionKind: selection.kind,
        triggerOpenedTabId,
      })
    ) {
      setSelectedTriggerId(null);
    }
  }

  // #385: the Pipeline Info peek overlay is tab-scoped and shadows the right
  // pane while open (rightPaneOwner gives it top precedence). Close it when the
  // active tab changes — selecting a different run/library-pipeline (left
  // panel), a Trigger opening its pipeline, or a TabBar switch all move
  // `editActiveTabId`. Canvas node/edge/note clicks already close it via
  // EditCanvas.onCloseInfo.
  //
  // Same render-time reset-on-change idiom as the #320 block above (adjust
  // state during render, not in an effect, to avoid painting one stale frame).
  // CRITICAL: advance `lastInfoTabId` UNCONDITIONALLY — if the advance were
  // gated on `infoPanelOpen`, the tracker would go stale while the overlay is
  // closed and spuriously close the NEXT overlay one frame after it opens on a
  // new tab (and gating would also re-fire this block every render).
  const [lastInfoTabId, setLastInfoTabId] = useState<string | null>(
    editActiveTabId,
  );
  if (lastInfoTabId !== editActiveTabId) {
    const closeInfo = shouldCloseInfoOnTabChange({
      prevTabId: lastInfoTabId,
      nextTabId: editActiveTabId,
      infoOpen: infoPanelOpen,
    });
    setLastInfoTabId(editActiveTabId); // UNCONDITIONAL — mirrors the #320 block
    if (closeInfo) setInfoPanelOpen(false);
  }

  const selectedTrigger =
    selectedTriggerId != null
      ? triggers.find((t) => t.id === selectedTriggerId) ?? null
      : null;

  // Prompt-required (#351): default true when the flag is absent (matches the
  // daemon default); false when the pipeline can't be found, so a dangling
  // reference shows no false "would be empty" caveat.
  const triggerPromptRequired = selectedTrigger
    ? (() => {
        const p = pipelines.find((pl) => pl.id === selectedTrigger.pipeline_id);
        return p ? p.prompt_required !== false : false;
      })()
    : false;

  // Which view owns the right-hand detail pane (#247). A selected Trigger now
  // wins over a persistent run-edit tab; the canvas-focus reconciliation above
  // clears `selectedTriggerId` the moment the canvas is touched again.
  const paneOwner = rightPaneOwner({
    triggerSelected: selectedTrigger != null,
    infoPanelOpen,
    hasEditTab,
  });

  // The Run-pane node. A node present in the pipeline (canvas) but absent from
  // the run's node map is genuinely pending: the event-sourced projection only
  // lists a node once it has been scheduled (NodeStarted / NodeWaiting / …), so
  // a not-yet-reached downstream node has no entry. On a live run, synthesize a
  // pending NodeState so the inspector renders NodeDetailPanel (with its
  // force-start Start button, #204) instead of the passive RunTabPlaceholder —
  // the daemon's force_spawn_node already accepts a node absent from run state.
  // Terminal runs and start/end pseudo-nodes stay null.
  const runNode: NodeState | null = (() => {
    if (selection.kind !== "node" || !selection.id || !selectedRun) return null;
    const existing = selectedRun.nodes[selection.id];
    if (existing) return existing;
    if (!isLiveRun(selectedRun.status)) return null;
    const def = selectedRun.node_defs?.find((d) => d.id === selection.id);
    if (!def || def.node_type === "start" || def.node_type === "end")
      return null;
    return {
      node_id: selection.id,
      status: "pending",
      iter: 0,
      started_at: null,
      completed_at: null,
      failure_reason: null,
      iterations: [],
    };
  })();

  return { paneOwner, selectedTrigger, triggerPromptRequired, runNode };
}
