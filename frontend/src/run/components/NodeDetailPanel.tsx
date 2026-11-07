import { useCallback, useMemo, useState } from "react";
import type { ScopedProvisioningRules } from "../../shared/generated/ScopedProvisioningRules";
import type { NodeState } from "../../shared/generated/NodeState";
import type { RunStatus } from "../../shared/generated/RunStatus";
import { useNodeRun } from "../store/useNodeRun";
import MarkdownArtifactModal from "./MarkdownArtifactModal";
import type { ArtifactSource } from "./MarkdownArtifactModal";
import { nodeReviewTarget } from "../model/runRefs";
import {
  childrenOfNode,
  countChildren,
  useRunChildren,
} from "../model/orchestration";
import NodeDetailHeader, { NodeProvisioning } from "./nodeDetail/NodeDetailHeader";
import NodeControls, { ActionVerdictBanner } from "./nodeDetail/NodeControls";
import NodeBanners from "./nodeDetail/NodeBanners";
import NodeVerdicts from "./nodeDetail/NodeVerdicts";
import { createPortal } from "react-dom";
import { useInspectorBarSlot } from "../../pipeline/components/inspectorBarSlot";
import { reviewUrl } from "../model/runRefs";
import "./inspector.css";
import DetailsPane from "./nodeDetail/DetailsPane";
import TerminalSection from "./nodeDetail/TerminalSection";
import RetryConfirmDialog from "./nodeDetail/RetryConfirmDialog";
import { isMarkCompletable, nodeSessionEnded } from "../model/nodeStatus";
import type { ModalState, TerminalView } from "./nodeDetail/types";
import { useNodeGestureRefusals } from "../store/useNodeGestureRefusals";
import { useFocusTab } from "../store/runSearchStore";
import { useOrchestrationFocusTab } from "../store/orchestrationFocus";

interface Props {
  node: NodeState;
  runId: string;
  /** The Run's name, on the footer of an artifact opened from here. */
  runName?: string | null;
  isArchived?: boolean;
  /** The Run's status: a paused Run takes no completion until resumed. */
  runStatus?: RunStatus;
  nodeName?: string | null;
  initialTerminalExpanded?: boolean;
  provisioningRepository?: string;
  inheritedProvisioning?: ScopedProvisioningRules[];
  provisioningGitRef?: string;
  /** #723: the node's frozen « Orchestrator » toggle (ADR-0064) — mounts the
   *  I/O | Orchestration tab pair and the pastilles. Absent ⇒ plain I/O pane,
   *  byte-identical to the pre-#723 panel. */
  isOrchestratorNode?: boolean;
  /** #723: open a child run listed in the Orchestration tab (App records the way back). */
  onOpenChildRun?: (childRunId: string) => void;
  /** #723: land on the Orchestration tab (the back arrow from a child run). */
  initialDetailTab?: "io" | "transcript" | "orchestration";
  /** ADR-0086: the node is an on-demand node — its Start is its ▶. */
  isOnDemand?: boolean;
}

export default function NodeDetailPanel({
  node,
  runId,
  runName = null,
  isArchived,
  runStatus,
  nodeName,
  initialTerminalExpanded,
  provisioningRepository = "",
  inheritedProvisioning,
  provisioningGitRef = "HEAD",
  isOrchestratorNode = false,
  onOpenChildRun,
  initialDetailTab = "io",
  isOnDemand = false,
}: Props) {
  const [modal, setModal] = useState<ModalState | null>(null);
  // #588: whether the inline terminal has a live PTY socket (the banner's hint
  // depends on it). `null` before the terminal reported — read as live, so the
  // frozen hint never flashes while the socket is still connecting.
  const [terminalLive, setTerminalLive] = useState<boolean | null>(null);
  // #870: whether this browser only watches the inline terminal.
  const [terminalSpectating, setTerminalSpectating] = useState(false);
  // #723 — an orchestrator NodeRun splits its lower pane into I/O | Orchestration.
  // I/O stays the default; the choice is per mounted node (remount = back to I/O).
  // `io`, `transcript`, `orchestration`, or the key of a `node/detail` tab of an
  // extension.
  const [detailTab, setDetailTab] = useState<string>(initialDetailTab);
  // The children read polls with the run view cadence (never pushed), whether or
  // not the tab is open — the tab-header pastilles count from the same read.
  const childrenData = useRunChildren(runId, isOrchestratorNode);
  const nodeChildren = childrenOfNode(childrenData, runId, node.node_id);
  const childCounts = countChildren(nodeChildren);
  // #749: what the Review shortcut opens, if anything.
  const reviewTarget = nodeReviewTarget(node);
  // Seed at mount only (no reactive effect on status): the issue trigger is
  // "clicking the node" (= selection / mount), not a live transition. A node
  // is `key`-ed by node_id at both mount sites, so selecting another terminated
  // node remounts → re-seeds → minimized; a node that settles under the user's
  // eyes is NOT folded reactively (that would be jarring — cf. #270 "agrandir =
  // explicit gesture"). Retry clears `minimized` explicitly (session revives).
  const [terminalView, setTerminalView] = useState<TerminalView>(() => {
    if (initialTerminalExpanded) return "expanded"; // seam #270/#129 (dead in prod)
    return nodeSessionEnded(node.status, isArchived) ? "minimized" : "split";
  });
  const [userSelectedIter, setUserSelectedIter] = useState<{
    nodeId: string;
    iter: number;
  } | null>(null);

  const selectedIter =
    userSelectedIter?.nodeId === node.node_id
      ? userSelectedIter.iter
      : node.iter;

  const setSelectedIter = useCallback(
    (iter: number) => {
      setUserSelectedIter({ nodeId: node.node_id, iter });
    },
    [node.node_id],
  );

  // A hit chosen in the Run's search opens this node's Transcript at its
  // iteration.
  const focusTranscript = useCallback(
    (iter: number) => {
      setDetailTab("transcript");
      setSelectedIter(iter);
    },
    [setSelectedIter],
  );
  useFocusTab(runId, node.node_id, focusTranscript);
  // The « N runs » row of the card opens the Orchestration tab.
  const focusOrchestration = useCallback(() => setDetailTab("orchestration"), []);
  useOrchestrationFocusTab(runId, node.node_id, focusOrchestration);

  // The retry handlers used to call `setTerminalView("split")` inline. The mode
  // stays presentational state here; it travels into `useNodeRun` as a *stable*
  // callback (`useCallback` on the setter, which never changes), so the retry
  // callbacks keep the identity they had.
  const showTerminalSplit = useCallback(() => setTerminalView("split"), [setTerminalView]);

  // Every read and command of this panel is iter-scoped, so the resolved
  // `selectedIter` is an argument: the panel owns the IterSelector's state and the
  // hook owns the orchestration. One owner, no second copy to keep in sync.
  const {
    promptText,
    inputs,
    outputs,
    live,
    markVerdict,
    releaseVerdict,
    actionVerdict,
    retryConfirm,
    stop,
    retry,
    confirmRetry,
    cancelRetry,
    start,
    markComplete,
    releaseCompletion,
    killStale,
    restartIteration,
  } = useNodeRun(runId, node, selectedIter, {
    isArchived,
    onRetryStarted: showTerminalSplit,
  });

  // CORE-42, CORE-205: the gestures the caller may not make here, greyed with why.
  const refused = useNodeGestureRefusals(runId, node.node_id);
  // The daemon would complete the node: the command field offers it on the same condition (paletteItems.ts).
  const markCompletable = isMarkCompletable(node.status, runStatus ?? (isArchived ? "archived" : "running"));

  const sessionName = `pdo-${runId}-${node.node_id}-iter-${selectedIter}`;
  const hasMultipleIters = (node.iterations?.length ?? 0) > 1;
  const showTerminal = node.status !== "pending";

  // The status of the iteration **on screen**, not the node's rollup (#617). They
  // agree on the latest iteration; they part when the IterSelector sits on an older
  // one, whose session was reaped long ago even though the node is running again.
  // The terminal reads this to decide whether to attach or to read the frozen pane.
  const selectedIterStatus =
    node.iterations?.find((i) => i.iter === selectedIter)?.status ?? node.status;
  const selectedIteration = node.iterations?.find((i) => i.iter === selectedIter);
  const completionReleased = selectedIteration?.completion_released === true;
  const isReleasing =
    releaseVerdict?.iter === selectedIter && releaseVerdict.kind === "pending";
  const canReleaseCompletion =
    selectedIteration?.interactive === true &&
    (selectedIterStatus === "running" || selectedIterStatus === "awaiting_user") &&
    !isArchived;
  // #588 (A3): a refused unreleased completion makes « Mark ready » the primary
  // call to action — a ring on the button, the banner links to it.
  // The latest iteration awaits whenever the node rollup says so and carries
  // the `awaiting` info — even if the iteration row itself still reads
  // `running` (a derived child wait, FP #793 finding 1).
  const nodeAwaitsOnLatestIter =
    selectedIter === node.iter && node.status === "awaiting_user" && node.awaiting != null;
  const releaseIsPrimary =
    canReleaseCompletion &&
    !completionReleased &&
    selectedIter === node.iter &&
    node.awaiting?.cause === "completion_not_released";

  // #369: the I/O poll (`setInputs`/`setOutputs`) re-renders this panel every
  // tick (1s live, 5s settled). Building the modal's `source` prop as an inline
  // object literal handed the child a fresh reference on every one of those
  // re-renders; the modal keyed a fetch effect on that reference, so it re-ran
  // `fetchNodeIO` + `setFiles(new array)` + `setFileIndex(0)` on every tick,
  // which momentarily emptied the body and unmounted/remounted the rendered
  // markdown (mermaid diagram → back to its empty `aria-busy` state → flicker).
  // Memoize on the structural keys so the reference stays stable across ticks
  // and only changes when the underlying identity actually does.
  const modalSource = useMemo<ArtifactSource | null>(() => {
    if (!modal) return null;
    return node.iterations && node.iterations.length > 1
      ? {
          kind: "iter-nav",
          nodeId: node.node_id,
          portKind: modal.portKind,
          iterations: node.iterations,
          initialIter: selectedIter,
        }
      : { kind: "static", files: modal.files };
  }, [modal, node.iterations, node.node_id, selectedIter]);

  // #369: a stable `onClose` (setModal is a stable useState setter, so no deps).
  // The panel re-renders on every I/O poll tick; an inline `() => setModal(null)`
  // handed the modal a fresh prop each tick, re-rendering it needlessly. A constant
  // identity lets the modal's memoised markdown props do their job (no diagram
  // remount, no flicker).
  const closeModal = useCallback(() => setModal(null), []);

  const controls = !isArchived ? (
    <NodeControls
      node={node}
      stop={stop}
      restartIteration={restartIteration}
      start={start}
      retry={retry}
      refused={refused}
      review={reviewTarget ? { href: reviewUrl(runId, reviewTarget.pair), label: reviewTarget.label } : null}
      completion={{
        canRelease: canReleaseCompletion,
        released: completionReleased,
        isReleasing,
        releaseIsPrimary,
        release: releaseCompletion,
        releaseRefused: refused.release,
        canMarkComplete: markCompletable,
        markCompleteRefused: refused.markComplete,
        markComplete,
      }}
    />
  ) : null;
  // Under the Run / Config tabs, the controls sit at the right of the tab bar
  // while the Run tab is on screen; elsewhere at the end of the header line.
  const barSlot = useInspectorBarSlot();

  return (
    <aside className="ri" data-testid="node-detail-panel">
      <NodeDetailHeader
        node={node}
        nodeName={nodeName}
        hasMultipleIters={hasMultipleIters}
        selectedIter={selectedIter}
        onSelectIter={setSelectedIter}
        statusLabel={isOnDemand && node.status === "pending" ? "On demand" : undefined}
        trailing={barSlot ? null : controls}
      />
      {barSlot?.element && barSlot.runActive && controls && createPortal(controls, barSlot.element)}

      <NodeProvisioning
        node={node}
        provisioningRepository={provisioningRepository}
        inheritedProvisioning={inheritedProvisioning}
        provisioningGitRef={provisioningGitRef}
      />

      <div className="ri-banners">
        {/* #487: the refusal of a Retry/Play or Start click, rendered AT the
            gesture. Before this the daemon's 409 was swallowed, so the click
            looked like it did nothing. */}
        {!isArchived && actionVerdict && <ActionVerdictBanner actionVerdict={actionVerdict} />}
        {!isArchived && (
          <NodeVerdicts
            status={node.status}
            selectedIter={selectedIter}
            completionReleased={completionReleased}
            releaseVerdict={releaseVerdict}
            markVerdict={markVerdict}
          />
        )}
        <NodeBanners
          node={node}
          isArchived={isArchived}
          selectedIter={selectedIter}
          selectedIterStatus={selectedIterStatus}
          completionReleased={completionReleased}
          nodeAwaitsOnLatestIter={nodeAwaitsOnLatestIter}
          terminalLive={terminalLive}
          terminalSpectating={terminalSpectating}
          canReleaseCompletion={canReleaseCompletion}
          isReleasing={isReleasing}
          releaseCompletion={releaseCompletion}
          markComplete={markComplete}
          onOpenChildRun={onOpenChildRun}
          killStale={killStale}
          restartIteration={restartIteration}
          retry={retry}
          refused={refused}
          markCompletable={markCompletable}
        />
      </div>

      <TerminalSection
        node={node}
        nodeName={nodeName}
        runId={runId}
        isArchived={isArchived}
        selectedIter={selectedIter}
        selectedIterStatus={selectedIterStatus}
        sessionName={sessionName}
        showTerminal={showTerminal}
        terminalView={terminalView}
        setTerminalView={setTerminalView}
        setTerminalLive={setTerminalLive}
        setTerminalSpectating={setTerminalSpectating}
        terminalSpectating={terminalSpectating}
        // #44: the enlarged terminal carries the panel's completion gestures.
        completion={{
          showGestures:
            !isArchived &&
            (selectedIterStatus === "awaiting_user" || nodeAwaitsOnLatestIter),
          canRelease: canReleaseCompletion,
          releaseRefused: refused.release,
          canMarkComplete: markCompletable,
          markCompleteRefused: refused.markComplete,
          released: completionReleased,
          isReleasing,
          releaseVerdict,
          markVerdict,
          release: releaseCompletion,
          markComplete,
        }}
        detailsPane={
          <DetailsPane
            node={node}
            runId={runId}
            isArchived={isArchived}
            isOrchestratorNode={isOrchestratorNode}
            nodeName={nodeName ?? null}
            detailTab={detailTab}
            onSelectTab={setDetailTab}
            childCounts={childCounts}
            nodeChildren={nodeChildren}
            onOpenChildRun={onOpenChildRun}
            inputs={inputs}
            outputs={outputs}
            live={live}
            promptText={promptText}
            onOpenFile={(portKind, portName, files, portType) =>
              setModal({ portName, files, portKind, portType })
            }
            selectedIter={selectedIter}
            onSelectIter={setSelectedIter}
            iterLive={
              !isArchived &&
              (selectedIterStatus === "running" || selectedIterStatus === "awaiting_user")
            }
          />
        }
      />

      {modal && modalSource && (
        <MarkdownArtifactModal
          runId={runId}
          runName={runName}
          portName={modal.portName}
          portType={modal.portType}
          source={modalSource}
          onClose={closeModal}
        />
      )}

      {retryConfirm && (
        <RetryConfirmDialog
          retryConfirm={retryConfirm}
          confirmRetry={confirmRetry}
          cancelRetry={cancelRetry}
        />
      )}
    </aside>
  );
}
