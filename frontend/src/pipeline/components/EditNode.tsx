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
import NodeRimHandles from "./NodeRimHandles";
import { NodeTypeIcon, IsolationMarker } from "./NodeTypeIcon";
import { NodeCard } from "./NodeCard";
import { useIsDropTarget } from "./DragHighlightContext";
import { anchorHandleId, landsByDrop } from "../../lib/anchorSide";
import { WIRE, WIRE_SOFT } from "../../lib/wiringColors";
import { Bookmark, Play, SlidersHorizontal, TriangleAlert } from "lucide-react";
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
  const isDropTarget = useIsDropTarget(id);
  // ADR-0081 (#31): what the `canvas/node` decorations mark.
  const decorated = useMemo(
    () => ({
      entity: { kind: "node", id: data.nodeId, name: data.label, attributes: {} },
      node: { type: data.nodeType, status: data.status },
    }),
    [data.nodeId, data.label, data.nodeType, data.status],
  );
  const reached = data.reached ?? false;
  // A reached start/end marker borrows the green "completed" cadre (border +
  // faint green fill) so the inline run view signals end-reached the same way
  // completed work nodes already do (issue #105). Otherwise keep the live status.
  const cardStatus: NodeStatus = reached ? "completed" : data.status;
  const iconColor =
    reached ? "text-st-done"
    : data.nodeType === "start" ? "text-acc"
    : data.nodeType === "end" ? "text-st-blocked"
    : "text-fg-3";
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
  // CORE-45: an Output intermédiaire is shown on the card but starts no wire —
  // only the ordinary outputs grow the rim a wire is drawn from.
  const intermediateOutputs = data.outputs.filter((p) => p.intermediate);
  const wireable = data.outputs.length - intermediateOutputs.length;

  return (
    <>
    <NodeCard
      status={cardStatus}
      selected={isSelected}
      style={{
        minWidth: 160,
        fontSize: "12px",
        // The dashes take a light, thicker stroke: the card's dimming would
        // otherwise fade them into the canvas.
        ...(awaitingPlay
          ? { opacity: 0.6, borderStyle: "dashed", borderWidth: 2, borderColor: "var(--color-fg-2)" }
          : null),
        // Amber is the wiring colour throughout (#844): the same family the
        // selected edge uses. Green is the app's accent and already means
        // something else on a card. The soft ring arms the gesture on rim hover;
        // the solid one says « releasing here lands the wire », and wins when
        // both apply.
        ...(rimHover
          ? { boxShadow: `0 0 0 1px var(--color-bg-1), 0 0 0 2.5px ${WIRE_SOFT}` }
          : null),
        ...(isWireTarget
          ? { boxShadow: `0 0 0 1px var(--color-bg-1), 0 0 0 2.5px ${WIRE}` }
          : null),
      }}
    >
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
          borderRadius: 6,
          transform: "none",
          border: "none",
          background: "transparent",
          opacity: isDropTarget ? 1 : 0,
        }}
      />
      {/* Anchor-by-drop-position (#168): an emergent work node also renders one
          invisible side-centre target handle PER SIDE. An incoming edge binds to
          the handle for its persisted `target_side`, so xyflow anchors the arrow
          and derives the arrival geometry from that side (no forced left->right).
          End grows them too (#840): its `result` is the edge's port, not a place
          on the card. */}
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
      {/* Slim card (#149): type icon + name + code/doc marker only. The node id
          and the amber interactive badge are intentionally dropped from the
          card. */}
      <div className="flex items-center gap-2">
        <span className="relative shrink-0">
          <NodeTypeIcon type={data.nodeType} size={14} className={iconColor} />
          {data.agentMode === "profile" && (
            <Bookmark data-testid="agent-mode-profile" size={8} className="absolute -bottom-1 -right-1 text-fg-2" />
          )}
          {data.agentMode === "custom" && (
            <SlidersHorizontal data-testid="agent-mode-custom" size={8} className="absolute -bottom-1 -right-1 text-fg-2" />
          )}
          {data.agentMode === "broken" && (
            <TriangleAlert data-testid="agent-mode-broken" size={8} className="absolute -bottom-1 -right-1 text-st-blocked" />
          )}
        </span>
        <span className="font-medium text-fg">{data.label}</span>
        <IsolationMarker isolated={data.isolated === true} />
        {data.onDemand && !data.inRun && (
          <span title="On demand — launched by your ▶ in a run" data-testid="on-demand-marker">
            <Play size={10} className="shrink-0 text-fg-4" />
          </span>
        )}
        {data.loopBadge && (
          <span
            data-testid={data.loopBadge.kind === "collection" ? "collection-badge" : "loop-badge"}
            className="ml-auto shrink-0 rounded border border-acc px-1.5 font-mono text-acc"
            style={{ fontSize: 10, lineHeight: "16px" }}
            title={
              data.loopBadge.kind === "collection"
                ? "collection region — fans out one lap per item"
                : "bounded loop region — one member"
            }
          >
            {data.loopBadge.text}
          </span>
        )}
      </div>
      {/* CORE-45: the Outputs intermédiaires, a row under the title — dashed,
          because no wire leaves them. Omitted entirely without one. */}
      {intermediateOutputs.length > 0 && (
        <div className="mt-1.5 flex flex-wrap items-center gap-1 pl-[22px]" data-testid="intermediate-outputs-row">
          {intermediateOutputs.map((p) => (
            <span
              key={p.name}
              data-testid="intermediate-output-chip"
              title={`${p.name}: intermediate output, shown while the node works, never wired`}
              className="rounded border border-dashed border-line-strong px-1 font-mono text-fg-4"
              style={{ fontSize: 9, lineHeight: "14px" }}
            >
              {p.name}
            </span>
          ))}
        </div>
      )}
      {/* #723 — orchestrator pastilles: a row under the title (the card grows
          a line); omitted entirely without children, so a node that never
          orchestrated stays byte-identical. */}
      {data.childCounts && totalChildren(data.childCounts) > 0 && (
        <div className="mt-1.5 flex items-center gap-2 pl-[22px]" data-testid="node-child-pills-row">
          <ChildCountPills counts={data.childCounts} size="xs" testId="node-child-pills" />
        </div>
      )}
      {/* ADR-0081 (#31) — the `canvas/node` decorations: a row under the
          title, omitted entirely without one. */}
      <ExtensionDecorations<CanvasNodeContext>
        location="canvas/node"
        extra={decorated}
        block
        className="mt-1.5 pl-[22px]"
      />
      {inputImages.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-1.5" data-testid="start-node-images">
          {inputImages.map((name) => (
            <div
              key={name}
              data-testid="start-node-image-chip"
              title={name}
              className="relative h-[34px] w-[46px] overflow-hidden rounded border border-line-strong bg-bg-1"
            >
              <div
                className="absolute inset-0"
                style={{
                  backgroundImage:
                    "repeating-linear-gradient(135deg, rgba(255,255,255,0.05) 0 5px, transparent 5px 10px)",
                }}
              />
              <div
                className="absolute inset-x-0 bottom-0 truncate bg-bg-0/70 px-1 font-mono text-fg-4"
                style={{ fontSize: "7.5px" }}
              >
                {name}
              </div>
            </div>
          ))}
        </div>
      )}
      {/* #844: the rim is the drag-source — no output dot any more. Rendered LAST
          and on a higher layer so it wins the pointer over the body target above;
          inside the rim the card still drags. A node that declares no output
          (the End marker), or only Outputs intermédiaires, starts no wire and
          grows no rim. */}
      {wireable > 0 && (
        <NodeRimHandles onRimHover={setRimHover} isConnectable={isConnectable} border={awaitingPlay ? 2 : 1} />
      )}
      {/* While the Run's transcripts are searched: the node's count of hits. */}
      {data.inRun && <SearchMatchBadge nodeId={data.nodeId} />}
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
        title={data.play === "start" ? "Start this node" : "Run this skipped node as if its branch had been taken"}
        onClick={(e) => {
          e.stopPropagation();
          data.onPlay?.();
        }}
        className="nodrag nopan absolute z-[4] flex cursor-pointer items-center justify-center rounded-full border border-acc bg-bg-1 text-acc shadow transition-colors hover:bg-acc hover:text-bg-1"
        style={{ top: -9, right: -9, width: 20, height: 20 }}
      >
        <Play size={10} />
      </button>
    )}
    </>
  );
}
