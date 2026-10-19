/**
 * Drawing a wire on the canvas (#844, #168): the connection, its source anchor, its drop side and its traced route.
 */

import type { ReactFlowInstance } from "@xyflow/react";
import type { EditorPipelineDef } from "../model/pipeline";
import type { EdgeDef } from "../../shared/generated/EdgeDef";
import { useCallback, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import { type Node, type Connection, type FinalConnectionState } from "@xyflow/react";
import { resolveTargetGeometrySide } from "../components/editEdgeDerivation";
import { CARD_HEIGHT, CARD_WIDTH } from "../../lib/nodePlacement";
import { anchorFromPoint, anchorPoint, anchorsByDropOnBody, landsByDrop, dropAnchor, sideFromRimHandle } from "../../lib/anchorSide";
import { drawnEdgeLayout } from "../../lib/drawnEdge";
import { pendingSource, resetWiringSession, wiringGesture, wiringTrace } from "../../lib/wiringSession";
import { droppedPath } from "../../lib/wiringGesture";
import { wireableOutputs } from "../../lib/edgePorts";
import { snapStepAtZoom } from "../../lib/wiringGrid";
// #723 — orchestrator pastilles on the run-view node card.

export function useCanvasWiring({
  pipeline,
  reactFlow,
  addEdgeToStore,
  updateEdge,
  setWiring,
  gridStep,
}: {
  pipeline: EditorPipelineDef | undefined;
  reactFlow: ReactFlowInstance;
  addEdgeToStore: (edge: EdgeDef) => void;
  updateEdge: (index: number, updates: Partial<EdgeDef>, opts?: { track?: boolean }) => void;
  setWiring: (on: boolean) => void;
  gridStep: number;
}) {
  const [isDraggingEdge, setIsDraggingEdge] = useState(false);
  const [hoveredNodeId, setHoveredNodeId] = useState<string | null>(null);

  // Index the just-drawn edge will occupy, captured at `onConnect` and consumed
  // by `onConnectEnd` to stamp the drop-position anchor side (#168). The edge is
  // appended, so its index is the edge count at draw time.
  const pendingEdgeIndexRef = useRef<number | null>(null);

  const onConnect = useCallback(
    (connection: Connection) => {
      if (!connection.source || !connection.target) return;
      if (!pipeline) return;
      const sourceNode = pipeline.nodes.find((n) => n.id === connection.source);
      const targetNode = pipeline.nodes.find((n) => n.id === connection.target);
      // #844: the drag starts on a RIM strip, which is a layout handle, not a
      // port. A new edge carries the FIRST DECLARED output (ADR-0073) and the
      // choice is revised in the panel's Outputs section. A structural source
      // handle (a declared port id) still names its own port. An Output
      // intermédiaire is never wired: the first ORDINARY output is the default.
      const sourcePort =
        (sideFromRimHandle(connection.sourceHandle) ? null : connection.sourceHandle) ??
        wireableOutputs(sourceNode)[0]?.name ??
        "out";
      // Inputs are emergent (#149): dropping on a node's body creates an input
      // named after the SOURCE document. A declared target handle is honoured
      // when one is named; otherwise the emergent name is inherited from the
      // source.
      // The body anchor handles (#168) are LAYOUT, not semantic ports — ignore
      // them here so the emergent input name still comes from the source.
      const declaredHandle = anchorsByDropOnBody(connection.targetHandle)
        ? null
        : connection.targetHandle;
      const targetPort = declaredHandle ?? targetNode?.inputs[0]?.name ?? sourcePort;

      pendingEdgeIndexRef.current = pipeline.edges.length;
      addEdgeToStore({
        source: { node: connection.source, port: sourcePort },
        target: { node: connection.target, port: targetPort },
      });
    },
    [addEdgeToStore, pipeline],
  );

  const onConnectStart = useCallback(
    (event: MouseEvent | TouchEvent, params: { nodeId: string | null; handleId: string | null }) => {
      setIsDraggingEdge(true);
      setWiring(true);
      // #844: the press position on the rim — the one thing xyflow does not pass
      // on later, and the whole basis of the source anchor. The connection line
      // and `onConnectEnd` read it back out of the gesture scratchpad.
      resetWiringSession();
      const side = sideFromRimHandle(params.handleId);
      const internal = params.nodeId ? reactFlow.getInternalNode(params.nodeId) : null;
      if (!side || !internal) return;
      const press = "touches" in event ? event.touches[0] : event;
      if (press?.clientX == null) return;
      const pressFlow = reactFlow.screenToFlowPosition({ x: press.clientX, y: press.clientY });
      const rect = {
        x: internal.internals.positionAbsolute.x,
        y: internal.internals.positionAbsolute.y,
        width: internal.measured?.width ?? CARD_WIDTH,
        height: internal.measured?.height ?? CARD_HEIGHT,
      };
      // CORE-88: the wire leaves from the pixel pressed (step 0), not from the
      // nearest grid line — the anchor is the press, like a drop's.
      const anchor = anchorFromPoint(pressFlow, rect, side, 0);
      pendingSource.anchor = anchor;
      pendingSource.point = anchorPoint(rect, anchor, anchor.side);
    },
    [reactFlow, setWiring],
  );
  const onConnectEnd = useCallback(
    (event: MouseEvent | TouchEvent, connectionState: FinalConnectionState) => {
      setIsDraggingEdge(false);
      setWiring(false);
      setHoveredNodeId(null);

      // Anchor the just-drawn edge on the target side nearest the drop (#168), and
      // pin the route the user actually drew (#844). The edge `onConnect` appended
      // is at `pendingEdgeIndexRef`.
      const edgeIndex = pendingEdgeIndexRef.current;
      pendingEdgeIndexRef.current = null;
      const sourceAnchor = pendingSource.anchor;
      const publishedTrace = wiringTrace.points;
      const gesture = wiringGesture.current;
      resetWiringSession();
      if (edgeIndex == null) return;

      const toNode = connectionState.toNode;
      if (!toNode) return;
      const targetDef = pipeline?.nodes.find((n) => n.id === toNode.id);

      // Where the user actually released, in FLOW coordinates. We read the raw
      // pointer (not `connectionState.to`, which xyflow snaps to a handle centre)
      // and convert it with `screenToFlowPosition` so it shares the target rect's
      // coordinate space. The #219 bug compared `connectionState.to` (rendered px,
      // zoom/pan-scaled) against a rect built from flow units, so `chooseAnchorSide`
      // got mismatched spaces and the arrow landed on the wrong side.
      const pointer = "changedTouches" in event ? event.changedTouches[0] : event;
      if (!pointer) return;
      const drop = reactFlow.screenToFlowPosition({ x: pointer.clientX, y: pointer.clientY });

      const rect = {
        x: toNode.internals.positionAbsolute.x,
        y: toNode.internals.positionAbsolute.y,
        width: toNode.measured.width ?? 0,
        height: toNode.measured.height ?? 0,
      };
      if (rect.width === 0 || rect.height === 0) return;

      // A node with a declared, fixed-side handle is never re-anchored. Work
      // nodes and End (#840) land where dropped. Keyed on
      // node TYPE: a work node carrying a vestigial declared `in` still anchors
      // (#175).
      const anchorsByDrop = targetDef != null && landsByDrop(targetDef.type);
      const targetAnchor = anchorsByDrop ? dropAnchor(drop, rect) : null;
      // The side the edge will actually RENDER with — the same answer
      // `deriveEditEdges` gives it on the next render. A declared-port target
      // persists no `target_side` (it has no say in one), so the route must be
      // enforced against the side its handle is DECLARED on, not against the
      // `left` that absence reads back as: enforcing on `left` saved a landing
      // into a border the wire never touches, and its phantom approach ended up
      // in the file as waypoints inside the card (#844, FP finding 2).
      const side =
        targetAnchor?.side ??
        (targetDef ? resolveTargetGeometrySide(targetDef, "left") : "left");

      // The traced waypoints ARE the route: the edge is born `mode: manual`
      // (#844). Rebuilt from THIS event's connection state, not taken from the
      // last pointer move, whose hover state lags one frame behind (see
      // `droppedPath`).
      const traced = gesture
        ? droppedPath(
            gesture,
            drop,
            toNode,
            connectionState.toHandle ?? null,
            connectionState.fromNode?.id ?? null,
            gridStep,
            snapStepAtZoom(gridStep, reactFlow.getZoom()).step,
          )
        : publishedTrace;
      // `drawnEdgeLayout` owns which fields a drop writes and which it
      // deliberately leaves absent.
      const updates = drawnEdgeLayout({
        traced,
        sourceAnchor,
        targetAnchor,
        targetSide: side,
        anchorsByDrop,
        step: gridStep,
      });
      if (Object.keys(updates).length === 0) return;
      // Untracked (ADR-0014 / #226): the preceding `addEdge` already pushed the
      // pre-edge snapshot, and this layout stamp is causally linked to it via
      // `pendingEdgeIndexRef`. Folding it into that one history entry makes a
      // single edge-draw gesture undo in one step (edge + route together).
      updateEdge(edgeIndex, updates, { track: false });
    },
    [pipeline, reactFlow, updateEdge, setWiring, gridStep],
  );
  const onNodeMouseEnter = useCallback((_: ReactMouseEvent, node: Node) => setHoveredNodeId(node.id), []);
  const onNodeMouseLeave = useCallback(() => setHoveredNodeId(null), []);
  return { isDraggingEdge, hoveredNodeId, onConnect, onConnectStart, onConnectEnd, onNodeMouseEnter, onNodeMouseLeave };
}
