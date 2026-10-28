import { useCallback } from "react";
import {
  BaseEdge,
  EdgeLabelRenderer,
  useReactFlow,
  type EdgeProps,
  type Edge,
} from "@xyflow/react";
import type { Point } from "../../lib/orthogonalRouter";
import { pathToSvg, pathMidpoint, segHandleStyle } from "../../lib/edgePath";
import {
  conditionLabelOffset,
  departureHeading,
  midpointAxis,
  outputLabelPlacement,
} from "../../lib/edgeLabels";
import { useEditStore } from "../store/editStore";
import { selectActiveTabLocked } from "../store/editState";
import type { EdgeAnchor } from "../../shared/generated/EdgeAnchor";
import type { EdgeDef } from "../../shared/generated/EdgeDef";
import type { EdgeWaypoint } from "../../shared/generated/EdgeWaypoint";
import type { PortSide } from "../../shared/generated/PortSide";
import { EDGE_LABELS_ABOVE_NODES, EDGE_LABELS_UNDER_NODES } from "./editEdgeDerivation";
import { useEdgeRoute } from "./useEdgeRoute";
import BrokenEdgeLabel from "./BrokenEdgeLabel";
import { useSegmentDrag } from "./useSegmentDrag";


export interface OrthogonalEdgeData extends Record<string, unknown> {
  edgeIndex: number;
  mode?: "auto" | "manual" | null;
  waypoints?: EdgeWaypoint[] | null;
  /**
   * The target card side the incoming arrow anchors on (#168). Layout, mirrors
   * `EdgeDef.target_side`. xyflow already derives the arrival geometry from the
   * bound side-handle's `Position`, so this is carried mainly for inspection and
   * round-tripping; the route arrives from this side, not always the left.
   */
  targetSide?: PortSide;
  /** The side the wire leaves its source by (#844): the anchor's side, else the
   *  carried output's declared side. Absent ⇒ `right`. */
  sourceSide?: PortSide;
  /** Where on the source card's border the wire leaves (#844). Absent ⇒ the
   *  middle of the departure side. */
  sourceAnchor?: EdgeAnchor | null;
  /** Where on the target card's side the arrow lands (#844). Absent ⇒ the middle
   *  of `targetSide`. */
  targetAnchor?: EdgeAnchor | null;
  isConditional: boolean;
  isElse: boolean;
  label?: string;
  strokeColor: string;
  dashed: boolean;
  /** The outputs this edge carries (ADR-0073 / #843) — one canvas label each. */
  ports?: string[];
  /** Resolved `show_output_labels` (#845): the per-edge toggle, else the
   *  count-derived default. Absent ⇒ off (an edge with no carried port to name). */
  showOutputLabels?: boolean;
  /** Pinned absolute positions of the output labels, keyed by port (#845). */
  outputLabelPos?: Record<string, EdgeWaypoint> | null;
  /** Pinned absolute position of the condition pill (#845). */
  conditionLabelPos?: EdgeWaypoint | null;
  /** First placement slot of this edge's output labels (#845): non-zero when
   *  an earlier edge leaves from the same point, so the two sets don't stack. */
  outputLabelSlot?: number;
  /** `below_nodes` (#845): the edge — and so its labels and handles — draws
   *  under the node cards. */
  belowNodes?: boolean;
  /** CORE-88: the carried ports its source no longer declares. Non-empty ⇒ the
   *  edge is « cassé »: red, dashed, named, and deletable from the canvas. */
  missingPorts?: string[];
}

/**
 * Orthogonal (right-angle) edge with manual-waypoint shaping (#154, design screen
 * 14) on the wiring grid (#844 / ADR-0072). Auto edges pathfind around other nodes
 * via `routeOrthogonal`, are snapped onto the lattice, and re-route for free when a
 * node moves. Selecting the edge reveals perpendicular-only segment handles (#178)
 * that snap to the grid — Shift frees them, neighbours are never re-snapped, and a
 * drag that aligns two segments merges the waypoint between them on release. The
 * reset back to auto lives in the edge detail panel.
 */
export default function OrthogonalEdge({
  id,
  sourceX,
  sourceY,
  targetX,
  targetY,
  source,
  target,
  sourcePosition,
  markerEnd,
  data,
}: EdgeProps<Edge<OrthogonalEdgeData>>) {
  const updateEdge = useEditStore((s) => s.updateEdge);
  const deleteEdge = useEditStore((s) => s.deleteEdge);
  // Selection drives the stroke color (#177). The store is the source of truth
  // the edge detail panel keys off, so reading it here (mirroring `EditNode`'s
  // own `isSelected` derivation) keeps the orange stroke and the open panel in
  // lockstep — and the store guarantees a single selection, so at most one edge
  // is orange at a time. It survives edge re-derivation too, unlike xyflow's
  // transient per-element `selected` flag, which is reset when edges are rebuilt.
  const selection = useEditStore((s) => s.selection);
  const setSelection = useEditStore((s) => s.setSelection);
  // ADR-0085: a Run followed in « pilotage » offers no route or label drag; a
  // click on a label still selects the edge.
  const locked = useEditStore(selectActiveTabLocked);
  const { screenToFlowPosition } = useReactFlow();

  const route = useEdgeRoute(source, target, sourceX, sourceY, targetX, targetY, data);
  const { points, handles, targetPt } = route;
  const d = pathToSvg(points);

  const edgeIndex = data?.edgeIndex;
  const onHandleDrag = useSegmentDrag(edgeIndex, route);

  // Every label write goes through here, so the `edgeIndex` guard is stated
  // once and the drag handlers below stay about the gesture.
  const patchEdge = useCallback(
    (updates: Partial<EdgeDef>) => {
      if (edgeIndex == null) return;
      updateEdge(edgeIndex, updates);
    },
    [edgeIndex, updateEdge],
  );

  /**
   * Free-drag a label to an absolute canvas position (#845).
   *
   * A label must not BLOCK the click on its edge (#845 AC), but it also has to
   * be grabbable — so it cannot simply be `pointer-events: none`. The two
   * gestures are told apart by travel: past 3 px it is a drag and the position
   * is written; below that it is a click, and the selection is handed to the
   * edge underneath exactly as if the stroke itself had been hit.
   *
   * Every move writes through `updateEdge` under the same coalesce key, so a
   * whole drag folds into ONE undo step (ADR-0014), like a node drag.
   */
  const onLabelDrag = useCallback(
    (commit: (p: Point) => void) => (e: React.PointerEvent) => {
      // Primary button only: without this the right-click that opens the edge's
      // context menu would also start a drag and silently move the label.
      if (e.button !== 0 || edgeIndex == null) return;
      e.stopPropagation();
      e.preventDefault();
      const start = { x: e.clientX, y: e.clientY };
      let moved = false;
      const move = (ev: PointerEvent) => {
        if (Math.abs(ev.clientX - start.x) + Math.abs(ev.clientY - start.y) > 3) moved = true;
        if (moved && !locked) commit(screenToFlowPosition({ x: ev.clientX, y: ev.clientY }));
      };
      const up = () => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", up);
        if (!moved) setSelection({ kind: "edge", id: null, edgeIndex });
      };
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", up);
    },
    [edgeIndex, locked, screenToFlowPosition, setSelection],
  );

  // `EdgeLabelRenderer` portals its children out of the edge's SVG, but a React
  // portal still bubbles React events up the REACT tree — straight into
  // xyflow's `onEdgeContextMenu`. Swallowing it here is what keeps a right-click
  // on a label from opening the edge's "Delete edge" menu from a spot the
  // pointer never touched the stroke at.
  const onLabelContextMenu = useCallback((ev: React.MouseEvent) => {
    ev.preventDefault();
    ev.stopPropagation();
  }, []);

  // Pastel orange when this edge is the selected one, grey otherwise (#177).
  // The override flows through to the condition pill border and segment handles
  // below, which both read `strokeColor`, so the whole edge reads as selected.
  const isSelected = selection.kind === "edge" && selection.edgeIndex === edgeIndex;
  const missingPorts = data?.missingPorts ?? [];
  const broken = missingPorts.length > 0;
  const strokeColor = isSelected
    ? "var(--selected-edge)"
    : broken
      ? "var(--color-st-failed)"
      : data?.strokeColor ?? "var(--text-3)";

  // Anchor the condition pill at the path's arc-length midpoint (#176) — the
  // median vertex drifts off-center on unbalanced routes and stacks sibling
  // pills on shared bends — nudged clear of the stroke (#845), because a pill
  // straddling a straight edge sits exactly on the midpoint segment handle and
  // eats the pointer events that handle needs. A dragged pill overrides both.
  const mid = pathMidpoint(points) ?? targetPt;
  const midOffset = conditionLabelOffset(midpointAxis(points, mid));
  const pinnedCondition = data?.conditionLabelPos ?? null;
  const labelPoint: Point = pinnedCondition ?? {
    x: mid.x + midOffset.x,
    y: mid.y + midOffset.y,
  };

  // On the wire, as in the design; pushed to the other side of it when a
  // condition pill sits at `mid + midOffset`, so neither hides the other.
  const brokenAt: Point = data?.label
    ? { x: mid.x - 2 * midOffset.x, y: mid.y - 2 * midOffset.y }
    : mid;

  // Output labels (#845): one per carried port, near the departure, alternating
  // around the base of the arrow until the author drags them elsewhere.
  const ports = data?.ports ?? [];
  const showOutputLabels = data?.showOutputLabels === true && ports.length > 0;
  const heading = departureHeading(sourcePosition as PortSide | undefined, points);
  const outputLabelPos = data?.outputLabelPos ?? null;
  const labelSlot = data?.outputLabelSlot ?? 0;
  // Everything this edge portals into the label layer stacks with the edge
  // (#845). The layer itself is not a stacking context, so this z-index
  // competes with the node cards directly: over them for an edge drawn above,
  // level with the edge — and so painted under the later node layer — for one
  // drawn under. One notch over the edge also keeps its hit-stroke from
  // swallowing the pointer aimed at a label or handle.
  const labelZ = data?.belowNodes ? EDGE_LABELS_UNDER_NODES : EDGE_LABELS_ABOVE_NODES;

  return (
    <>
      <BaseEdge
        id={id}
        path={d}
        markerEnd={markerEnd}
        style={{
          stroke: strokeColor,
          strokeWidth: isSelected ? 2.5 : 1.5,
          strokeDasharray: broken ? "4 3" : data?.dashed ? "6 3" : undefined,
        }}
      />
      {/* Wide invisible hit area to make click-to-select forgiving. */}
      <path
        d={d}
        fill="none"
        stroke="transparent"
        strokeWidth={14}
        style={{ pointerEvents: "stroke", cursor: "pointer" }}
        data-testid={`orthogonal-edge-hit-${id}`}
      />
      <EdgeLabelRenderer>
        {/* Output labels (#845) — one per carried port, near the departure.
            Deliberately unlike the condition pill: a small square mono tag on a
            filled `bg-3`, hairline `line` border, no coloured outline, so the
            two kinds of label are never read as the same thing. */}
        {showOutputLabels &&
          ports.map((port, i) => {
            const place = outputLabelPlacement(labelSlot + i, heading.axis, heading.dir);
            const pinned = outputLabelPos?.[port];
            const x = pinned ? pinned.x : points[0].x + place.x;
            const y = pinned ? pinned.y : points[0].y + place.y;
            // A pinned label is centred on its own position — the drag put it
            // where the cursor was, so that is the point the author aimed at.
            // An un-pinned one is anchored by its near edge and grows away from
            // the card, or a long port name vanishes under it.
            const shiftX = pinned ? "-50%" : place.align === "start" ? "0%" : "-100%";
            return (
              <div
                key={port}
                className="nodrag nopan"
                data-testid={`edge-output-label-${id}-${port}`}
                onPointerDown={onLabelDrag((p) =>
                  patchEdge({
                    output_label_pos: {
                      ...(outputLabelPos ?? {}),
                      [port]: { x: Math.round(p.x), y: Math.round(p.y) },
                    },
                  }),
                )}
                onContextMenu={onLabelContextMenu}
                style={{
                  position: "absolute",
                  transform: `translate(${shiftX}, -50%) translate(${x}px, ${y}px)`,
                  // `EdgeLabelRenderer` is `pointer-events: none` and children
                  // do NOT inherit an opt-in: without this the label is not
                  // merely click-through, it is invisible to the mouse — and
                  // what looks like a drag is the pane panning underneath.
                  pointerEvents: "all",
                  zIndex: labelZ,
                  fontFamily: "var(--font-code, monospace)",
                  fontSize: 9.5,
                  lineHeight: 1.3,
                  whiteSpace: "nowrap",
                  cursor: "grab",
                  color: "var(--text-2)",
                  background: "var(--surface-2)",
                  border: "1px solid var(--line)",
                  borderRadius: 2,
                  padding: "1px 4px",
                }}
              >
                {port}
              </div>
            );
          })}
        {broken && (
          <BrokenEdgeLabel
            edgeId={id}
            ports={missingPorts}
            at={brokenAt}
            zIndex={labelZ}
            onDelete={!locked && edgeIndex != null ? () => deleteEdge(edgeIndex) : undefined}
            onContextMenu={onLabelContextMenu}
          />
        )}
        {/* Conditional pill (ADR-0011) — always visible, draggable since #845. */}
        {data?.label && (
          <div
            className="nodrag nopan"
            data-testid={`edge-condition-label-${id}`}
            onPointerDown={onLabelDrag((p) =>
              patchEdge({
                condition_label_pos: { x: Math.round(p.x), y: Math.round(p.y) },
              }),
            )}
            onContextMenu={onLabelContextMenu}
            style={{
              position: "absolute",
              transform: `translate(-50%, -50%) translate(${labelPoint.x}px, ${labelPoint.y}px)`,
              fontFamily: "var(--font-code, monospace)",
              fontSize: 10,
              color: "var(--text)",
              background: "var(--surface-1)",
              border: `1px solid ${strokeColor}`,
              borderRadius: 6,
              padding: "3px 6px",
              // Grabbable, not click-through (#845): a plain click still hands
              // the selection to the edge, so the pill never blocks it.
              pointerEvents: "all",
              zIndex: labelZ,
              cursor: "grab",
              whiteSpace: "nowrap",
            }}
          >
            {data.label}
          </div>
        )}
        {/* Perpendicular-only segment handles, visible while the edge is
            selected (#178) — never on mere hover, and gone on deselect even
            for manual-mode edges. No `onContextMenu` action: right-click on a
            handle does NOTHING now (#844) — a waypoint goes away by being
            dragged into alignment, not by a hidden menu. */}
        {isSelected &&
          !locked &&
          handles.map((h) => (
            <div
              key={h.segmentIndex}
              className="nodrag nopan"
              data-testid={`edge-seg-handle-${id}-${h.segmentIndex}`}
              onPointerDown={onHandleDrag(h.segmentIndex, h.orientation)}
              // `EdgeLabelRenderer` portals its children out of the edge's SVG,
              // but a React portal still bubbles React events up the REACT tree —
              // straight into xyflow's `onEdgeContextMenu`. Swallowing it here is
              // what makes right-click on a handle do nothing AT ALL, rather than
              // quietly opening the edge's own menu.
              onContextMenu={(ev) => {
                ev.preventDefault();
                ev.stopPropagation();
              }}
              style={{ ...segHandleStyle(h.x, h.y, h.orientation, strokeColor), zIndex: labelZ }}
            />
          ))}
      </EdgeLabelRenderer>
    </>
  );
}
