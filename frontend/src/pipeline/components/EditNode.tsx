/**
 * A node card on the edit canvas: its type icon, ports, rim handles, run
 * status and orchestrator pastilles.
 */

import { useMemo, useState } from "react";
import ExtensionDecorations from "../../shared/extensions/ExtensionDecorations";
import type { CanvasNodeContext } from "../../shared/extensions/contract";
import { Handle, Position, useConnection, type Node, type NodeProps } from "@xyflow/react";
import type { LoopKind } from "../../shared/generated/LoopKind";
import type { NodeStatus } from "../../shared/generated/NodeStatus";
import type { NodeType } from "../../shared/generated/NodeType";
import type { PortBrief } from "../../shared/generated/PortBrief";
import type { PortSide } from "../../shared/generated/PortSide";
import { useEditStore } from "../store/editStore";
import { useSelectedByOther } from "../store/useDraftPresence";
import { presenceColor } from "../model/presence";
import { SelectedByLabel } from "./ParticipantAvatar";
import NodeRimHandles from "./NodeRimHandles";
import { NodeTypeIcon, IsolationMarker } from "./NodeTypeIcon";
import { NodeCard } from "./NodeCard";
import { useIsDropTarget } from "./DragHighlightContext";
import { anchorHandleId, landsByDrop } from "../../lib/anchorSide";
import {
  ArrowDownToLine,
  ArrowUpFromLine,
  Bookmark,
  Bot,
  CircleCheck,
  Coins,
  GitFork,
  Play,
  Repeat,
  SlidersHorizontal,
  SquareTerminal,
  Timer,
  TriangleAlert,
  type LucideIcon,
} from "lucide-react";
import { StatusIcon } from "../../shared/ui/kit";
import { formatDuration } from "../../run/model/runDuration";
import { formatCostAmount } from "../../run/model/costLabel";
import { useNow } from "../../run/model/iterationDuration";
import type { NodeCost } from "../../shared/generated/NodeCost";
import type { NodeAgentLabel } from "../model/nodeAgent";
// #723 — orchestrator pastilles on the run-view node card.
import { ChildCountPills } from "../../run/components/OrchestrationTab";
import { totalChildren } from "../../run/model/orchestration";
import type { ChildCounts } from "../../run/model/orchestration";
import SearchMatchBadge from "../../run/components/runSearch/SearchMatchBadge";
// The four emergent body anchor handles (#168), each pinned to its side-centre
// with the matching xyflow `Position` so a bound incoming edge arrives from that
// side. `transform` re-centres the 1px handle on the edge midpoint.
const ANCHOR_HANDLE_SIDES: { side: PortSide; position: Position; style: React.CSSProperties }[] = [
  { side: "left", position: Position.Left, style: { left: 0, top: "50%", transform: "translateY(-50%)" } },
  { side: "right", position: Position.Right, style: { right: 0, top: "50%", transform: "translateY(-50%)" } },
  { side: "top", position: Position.Top, style: { top: 0, left: "50%", transform: "translateX(-50%)" } },
  { side: "bottom", position: Position.Bottom, style: { bottom: 0, left: "50%", transform: "translateX(-50%)" } },
];

interface EditNodeData {
  label: string;
  nodeId: string;
  nodeType: NodeType;
  inputs: PortBrief[];
  outputs: PortBrief[];
  interactive: boolean;
  status: NodeStatus;
  // True only for start/end markers on a completed run, or one waiting at End
  // for its started nodes — see `markerReached`.
  reached?: boolean;
  // Filenames of images uploaded with the run's input. Only the start marker
  // surfaces these (issue #145); undefined/empty on every other node.
  inputImages?: string[];
  // Compact badge when this node is the single member of a loop region: a
  // collection (`⇉ ...`, #151) or a single-member bounded loop (`↻ ...`, #173).
  // Absent on non-member nodes and on multi-member regions (boxed instead).
  loopBadge?: { text: string; kind: LoopKind };
  agentMode?: "inherit" | "profile" | "custom" | "broken";
  /** #723: the node carries the « Orchestrator » toggle (ADR-0064). */
  orchestrator?: boolean;
  /** #723: child-run counters of an orchestrator node (run view only). */
  childCounts?: ChildCounts;
  /** ADR-0086: the node carries the « on demand » option. */
  onDemand?: boolean;
  /** The card is drawn for a Run (not a template). */
  inRun?: boolean;
  /** ADR-0086: what the card's ▶ does, when it has one (run view only). */
  play?: "start" | "retry";
  onPlay?: () => void;
  /** What runs the node, for the card's meta grid. */
  agent?: NodeAgentLabel;
  /** The node in the Run: its iteration, its time and its cost. */
  run?: {
    iter: number;
    iterations: number;
    startedAt: string | null;
    completedAt: string | null;
    cost: NodeCost | null;
  };
  [key: string]: unknown;
}

// Exported for unit tests; co-located with the canvas it renders.
// `isConnectable` mirrors the canvas's `nodesConnectable` (false while a Run is
// followed or archived, ADR-0085): the handles then drop their wiring cue.
export function EditNode({ data, id, selected, isConnectable = true }: NodeProps<Node<EditNodeData>>) {
  const selection = useEditStore((s) => s.selection);
  // #844: hovering the rim arms the gesture — a subtle AMBER ring says « a wire
  // starts here », the crosshair on the strip itself says how.
  const [rimHover, setRimHover] = useState<PortSide | null>(null);
  // #844: the drop-target ring. Read from the LIVE connection rather than from a
  // hover handler, so the card lights up exactly when releasing here would land
  // the wire — which is the same answer `onConnectEnd` will give.
  const isWireTarget = useConnection(
    (c) => c.inProgress && c.toNode?.id === id && c.fromNode?.id !== id,
  );
  // OR-in xyflow's own `selected` (#232) so every node in a multi-select group
  // lights the accent ring during a drag, not just the last-clicked one the
  // Zustand single-selection tracks.
  const isSelected = selected || (selection.kind === "node" && selection.id === id);
  // ADR-0088: another participant of the shared draft selected this node.
  const selectedBy = useSelectedByOther("node", id);
  const selectedByColor = selectedBy ? presenceColor(selectedBy) : null;
  const isDropTarget = useIsDropTarget(id);
  // ADR-0081 (#31): what the `canvas/node` decorations mark.
  const decorated = useMemo(
    () => ({
      entity: { kind: "node", id: data.nodeId, name: data.label, attributes: {} },
      node: { type: data.nodeType, status: data.status },
    }),
    [data.nodeId, data.label, data.nodeType, data.status],
  );
  // A reached start/end marker wears the « done » border and, on End, a check,
  // so the run view signals end-reached like completed work nodes (#105).
  const reached = data.reached ?? false;
  // Images uploaded with the run's input ride along on the Start marker only
  // (issue #145). The canvas shows a compact, filename-tagged strip; the full
  // thumbnails live in the StartInspector.
  const inputImages =
    data.nodeType === "start" ? (data.inputImages ?? []) : [];

  // Work nodes and the End marker (#840) anchor incoming edges by drop
  // position, on any border. Keyed on node TYPE so a work node carrying a
  // vestigial declared `in` still anchors by drop (#175) rather than being
  // mistaken for a fixed-side declared port.
  const emergent = landsByDrop(data.nodeType);
  // ADR-0086: an on-demand node the Run never launched is drawn greyed — no
  // status of its own, it waits for the user's ▶.
  const awaitingPlay = data.onDemand === true && data.inRun === true && data.status === "pending";

  const handles = (
    <>
      {/* Emergent inputs (#149): NO input dots. An incoming arrow lands anywhere
          on the node body. A single invisible target handle covers the card and
          carries the drop highlight. A lands-by-drop node (work nodes, End since
          #840) renders it id-less and binds incoming edges to the per-side
          anchors below; any other declared input keeps its handle id and lands
          on the left (a port has no side). */}
      <Handle
        id={emergent ? undefined : data.inputs[0]?.name}
        type="target"
        position={Position.Left}
        isConnectable={isConnectable}
        isConnectableStart={false}
        className={`emergent-body-target${isDropTarget ? " is-drop" : ""}`}
        style={{
          position: "absolute",
          inset: 0,
          width: "100%",
          height: "100%",
          borderRadius: 2,
          transform: "none",
          border: "none",
          background: "transparent",
          opacity: isDropTarget ? 1 : 0,
        }}
      />
      {/* Anchor-by-drop-position (#168): an emergent work node also renders one
          invisible side-centre target handle PER SIDE. An incoming edge binds to
          the handle for its persisted `target_side`, so xyflow anchors the arrow
          and derives the arrival geometry from that side. End grows them too
          (#840): its `result` is the edge's port, not a place on the card. */}
      {emergent &&
        ANCHOR_HANDLE_SIDES.map(({ side, position, style }) => (
          <Handle
            key={`anchor-${side}`}
            id={anchorHandleId(side)}
            type="target"
            position={position}
            isConnectable={isConnectable}
            isConnectableStart={false}
            className="emergent-anchor-target"
            style={{
              position: "absolute",
              width: 1,
              height: 1,
              border: "none",
              background: "transparent",
              opacity: 0,
              ...style,
            }}
          />
        ))}
    </>
  );
  // #844: the rim is the drag-source. Rendered LAST and on a higher layer so it
  // wins the pointer over the body target above; inside the rim the card still
  // drags. A node that declares no output (the End marker) grows no rim.
  const rim = data.outputs.length > 0 && (
    <NodeRimHandles onRimHover={setRimHover} isConnectable={isConnectable} border={1} />
  );
  // ADR-0088: another participant of the shared draft selected this node.
  const presenceStyle =
    selectedByColor && !isSelected
      ? { borderColor: selectedByColor, boxShadow: `0 0 0 1px ${selectedByColor}` }
      : undefined;

  if (data.nodeType === "start" || data.nodeType === "end") {
    const start = data.nodeType === "start";
    return (
      <div
        data-testid="node-card"
        data-kind={data.nodeType}
        data-reached={reached || undefined}
        data-selected={isSelected || undefined}
        data-rim={rimHover || undefined}
        data-wire={isWireTarget || undefined}
        className="cv-mk"
        style={presenceStyle}
        title={start ? "Run start" : reached ? "Run end · reached" : "Run end"}
      >
        {selectedBy && <SelectedByLabel participant={selectedBy} />}
        {handles}
        <NodeTypeIcon type={data.nodeType} size={11} className="cv-mk__ic" />
        <span>{data.label}</span>
        {reached && !start && <CircleCheck size={12} strokeWidth={1.5} className="cv-mk__ok" aria-label="Reached" />}
        {inputImages.length > 0 && <InputImages names={inputImages} />}
        {/* ADR-0081 (#31) — the `canvas/node` decorations, under the marker. */}
        <ExtensionDecorations<CanvasNodeContext>
          location="canvas/node"
          extra={decorated}
          block
          className="cv-mk__deco"
        />
        {rim}
      </div>
    );
  }

  const inRun = data.inRun === true;
  const run = data.run;
  const typeIcon = (
    <NodeTypeIcon
      type={data.nodeType}
      interactive={data.interactive}
      orchestrator={data.orchestrator === true}
      size={inRun ? 12 : 14}
      className="cv-n__type"
    />
  );
  return (
    <>
    <NodeCard
      status={inRun ? data.status : null}
      selected={isSelected}
      idle={awaitingPlay}
      rimHover={rimHover != null}
      wireTarget={isWireTarget}
      style={presenceStyle}
    >
      {selectedBy && <SelectedByLabel participant={selectedBy} />}
      {handles}
      <div className="cv-n__head">
        {/* In a Run the status icon leads; on a template, the type icon. */}
        <span className="cv-n__lead">
          {inRun ? <StatusIcon kind="node" status={data.status} /> : typeIcon}
          {data.agentMode === "profile" && (
            <Bookmark data-testid="agent-mode-profile" size={8} className="cv-n__mode" aria-label="Agent profile" />
          )}
          {data.agentMode === "custom" && (
            <SlidersHorizontal data-testid="agent-mode-custom" size={8} className="cv-n__mode" aria-label="Custom agent" />
          )}
          {data.agentMode === "broken" && (
            <TriangleAlert data-testid="agent-mode-broken" size={8} className="cv-n__mode" data-broken aria-label="Missing agent profile" />
          )}
        </span>
        <span className="cv-n__name" title={data.label}>
          {data.label}
        </span>
        <IsolationMarker isolated={data.isolated === true} />
        {data.onDemand && !inRun && (
          <span className="cv-n__flag" title="On demand" data-testid="on-demand-marker">
            <Play size={10} strokeWidth={1.5} aria-label="On demand" />
          </span>
        )}
        {data.loopBadge && (
          <span
            data-testid={data.loopBadge.kind === "collection" ? "collection-badge" : "loop-badge"}
            className="cv-n__loop"
            title={data.loopBadge.kind === "collection" ? "Collection region" : "Loop region"}
          >
            {data.loopBadge.text}
          </span>
        )}
        {inRun && typeIcon}
      </div>
      <NodeMeta data={data} run={run} />
      {/* #723 — orchestrator pastilles: a row under the grid; omitted entirely
          without children, so a node that never orchestrated stays clean. */}
      {data.childCounts && totalChildren(data.childCounts) > 0 && (
        <div className="cv-n__row" data-testid="node-child-pills-row">
          <ChildCountPills counts={data.childCounts} size="pill" testId="node-child-pills" />
        </div>
      )}
      {/* ADR-0081 (#31) — the `canvas/node` decorations: a row under the
          grid, omitted entirely without one. */}
      <ExtensionDecorations<CanvasNodeContext>
        location="canvas/node"
        extra={decorated}
        block
        className="cv-n__row"
      />
      {rim}
      {/* While the Run's transcripts are searched: the node's count of hits. */}
      {inRun && <SearchMatchBadge nodeId={data.nodeId} />}
    </NodeCard>
    {/* ADR-0086: the ▶ — start an on-demand node, or run a skipped branch as
        if it had been taken. Outside the card so the greyed card does not dim
        it; `nodrag` keeps the click from moving the node. */}
    {data.play && data.onPlay && (
      <button
        type="button"
        data-testid="node-play-btn"
        data-play={data.play}
        aria-label={data.play === "start" ? `Start ${data.label}` : `Run ${data.label} again`}
        title={data.play === "start" ? "Start" : "Run as if taken"}
        onClick={(e) => {
          e.stopPropagation();
          data.onPlay?.();
        }}
        className="nodrag nopan cv-n__play"
      >
        <Play size={10} strokeWidth={1.75} aria-hidden />
      </button>
    )}
    </>
  );
}

/** Images uploaded with the Run's input, named, on the Start marker (#145). */
function InputImages({ names }: { names: string[] }) {
  return (
    <div className="cv-n__images" data-testid="start-node-images">
      {names.map((name) => (
        <div key={name} data-testid="start-node-image-chip" title={name} className="cv-n__img">
          <span>{name}</span>
        </div>
      ))}
    </div>
  );
}

/** One cell of the card's meta grid: an icon, a value, the name in the tooltip. */
function Cell({ icon: Icon, title, value }: { icon: LucideIcon; title: string; value: string | null | undefined }) {
  return (
    <span className="cv-n__cell" title={value ? `${title}: ${value}` : title}>
      <Icon size={11} strokeWidth={1.5} aria-hidden />
      <span>{value || "·"}</span>
    </span>
  );
}

/**
 * The card's meta grid, two columns: what runs the node, then on a template
 * its inputs, worktree and outputs, in a Run its iteration, time and cost.
 */
function NodeMeta({ data, run }: { data: EditNodeData; run: EditNodeData["run"] }) {
  const live = run != null && (data.status === "running" || data.status === "awaiting_user");
  const now = useNow(live && run?.completedAt == null);
  const agent = data.agent ?? { short: "", title: "Agent" };
  const AgentIcon = data.nodeType === "script" ? SquareTerminal : Bot;
  if (data.inRun !== true) {
    return (
      <div className="cv-n__grid">
        <Cell icon={AgentIcon} title={agent.title} value={agent.short} />
        <Cell icon={ArrowDownToLine} title="Inputs" value={data.inputs.map((p) => p.name).join(", ")} />
        <Cell icon={GitFork} title="Worktree" value={data.isolated === true ? "isolated" : "run"} />
        <Cell icon={ArrowUpFromLine} title="Outputs" value={data.outputs.map((p) => p.name).join(", ")} />
      </div>
    );
  }
  const elapsed =
    run?.startedAt != null
      ? formatDuration(
          (run.completedAt ? Date.parse(run.completedAt) : live ? now : Number.NaN) - Date.parse(run.startedAt),
        )
      : null;
  const cost =
    run?.cost && run.cost.usd != null
      ? formatCostAmount(run.cost.usd, run.cost.partial, run.cost.form !== "reported")
      : null;
  const iter = run && run.iter > 0 ? (data.loopBadge || run.iterations > 1 ? `×${run.iter}` : String(run.iter)) : null;
  return (
    <div className="cv-n__grid">
      <Cell icon={AgentIcon} title={agent.title} value={agent.short} />
      <Cell icon={Repeat} title="Iteration" value={iter} />
      <Cell icon={Timer} title="Elapsed" value={elapsed} />
      <Cell icon={Coins} title="Cost" value={cost} />
    </div>
  );
}
