import { useMemo, useCallback, useRef } from "react";
import {
  BaseEdge,
  EdgeLabelRenderer,
  useStore,
  useReactFlow,
  type EdgeProps,
  type Edge,
} from "@xyflow/react";
import { routeOrthogonal, type Point, type Rect } from "../lib/orthogonalRouter";
import {
  pathToSvg,
  pathMidpoint,
  segmentHandles,
  segHandleStyle,
} from "../lib/edgePath";
import {
  anchorPoint,
  dragSegmentKeepingRun,
  enforcePerpendicularEnds,
  landingLeg,
  storableWaypoints,
} from "../lib/anchorSide";
import {
  conditionLabelOffset,
  departureHeading,
  midpointAxis,
  outputLabelPlacement,
} from "../lib/edgeLabels";
import {
  mergeAlignedSegments,
  snapPolyline,
  WIRING_GRID_STEP,
} from "../lib/wiringGrid";
import { useWiringStore } from "../stores/wiringStore";
import { useEditStore } from "../stores/editStore";
import type { EdgeAnchor, EdgeDef, EdgeWaypoint, PortSide } from "../types";
import { EDGE_LABELS_ABOVE_NODES, EDGE_LABELS_UNDER_NODES } from "./editNodeDerivation";

/**
 * The lattice an AUTO route snaps to: the canvas's own, through the origin —
 * deliberately NOT the wiring session's current origin.
 *
 * A gesture re-origins the lattice on its own tip when Shift is released
 * (ADR-0072), which is right for the wire being drawn and wrong for every other
 * edge on the canvas: reading the session origin here made every auto route on
 * screen jump a few pixels the moment someone started drawing a wire somewhere
 * else. An auto route belongs to no gesture, so it snaps to the shared lattice
 * and stays put.
 */
const CANVAS_LATTICE: Point = { x: 0, y: 0 };

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
  // Selection drives the stroke color (#177). The store is the source of truth
  // the edge detail panel keys off, so reading it here (mirroring `EditNode`'s
  // own `isSelected` derivation) keeps the orange stroke and the open panel in
  // lockstep — and the store guarantees a single selection, so at most one edge
  // is orange at a time. It survives edge re-derivation too, unlike xyflow's
  // transient per-element `selected` flag, which is reset when edges are rebuilt.
  const selection = useEditStore((s) => s.selection);
  const setSelection = useEditStore((s) => s.setSelection);
  const { screenToFlowPosition } = useReactFlow();

  // Obstacle rects: every node except this edge's own source and target. Read
  // from the flow store so a node move re-renders the edge with fresh bounds.
  const obstacles = useStore(
    useCallback(
      (s): Rect[] => {
        const rects: Rect[] = [];
        for (const [, node] of s.nodeLookup) {
          if (node.id === source || node.id === target) continue;
          const w = node.measured?.width ?? node.width ?? 0;
          const h = node.measured?.height ?? node.height ?? 0;
          if (w === 0 || h === 0) continue;
          rects.push({ x: node.internals.positionAbsolute.x, y: node.internals.positionAbsolute.y, width: w, height: h });
        }
        return rects;
      },
      [source, target],
    ),
  );

  // The source and target card rects, so the persisted anchors can be turned into
  // absolute points. xyflow hands over `sourceX/Y` and `targetX/Y` — the CENTRES
  // of the bound handles — and an anchored edge is precisely one that does not
  // leave from there.
  const endpointRects = useStore(
    useCallback(
      (s): { source: Rect | null; target: Rect | null } => {
        const read = (nodeId: string): Rect | null => {
          const n = s.nodeLookup.get(nodeId);
          if (!n) return null;
          const width = n.measured?.width ?? n.width ?? 0;
          const height = n.measured?.height ?? n.height ?? 0;
          if (width === 0 || height === 0) return null;
          return {
            x: n.internals.positionAbsolute.x,
            y: n.internals.positionAbsolute.y,
            width,
            height,
          };
        };
        return { source: read(source), target: read(target) };
      },
      [source, target],
    ),
    (a, b) =>
      JSON.stringify(a.source) === JSON.stringify(b.source) &&
      JSON.stringify(a.target) === JSON.stringify(b.target),
  );

  const sourceAnchor = data?.sourceAnchor ?? null;
  const targetAnchor = data?.targetAnchor ?? null;
  const srcRect = endpointRects.source;
  const tgtRect = endpointRects.target;
  const srcAnchored =
    srcRect && sourceAnchor ? anchorPoint(srcRect, sourceAnchor, sourceAnchor.side) : null;
  const tgtAnchored =
    tgtRect && targetAnchor ? anchorPoint(tgtRect, targetAnchor, targetAnchor.side) : null;
  // Rounded to whole flow pixels: xyflow's handle centres are sub-pixel and differ
  // by a fraction, enough to turn a dead-straight wire into a three-segment path
  // with an invisible 0.2px jog in it — and two spurious drag handles.
  const sourcePt: Point = {
    x: Math.round(srcAnchored?.x ?? sourceX),
    y: Math.round(srcAnchored?.y ?? sourceY),
  };
  const targetPt: Point = {
    x: Math.round(tgtAnchored?.x ?? targetX),
    y: Math.round(tgtAnchored?.y ?? targetY),
  };
  const mode = data?.mode;
  const waypoints = data?.waypoints;

  // The sides the wire leaves and arrives on. An edge drawn before #844 has no
  // source anchor and leaves by its output's declared side, like the shipped
  // canvas always did (the dot sat there).
  const srcSide: PortSide = sourceAnchor?.side ?? data?.sourceSide ?? "right";
  const tgtSide: PortSide = targetAnchor?.side ?? data?.targetSide ?? "left";
  const leg = landingLeg(WIRING_GRID_STEP);

  const points: Point[] = useMemo(() => {
    if (mode === "manual" && waypoints && waypoints.length > 0) {
      // NO `reanchorWaypoints` here. That helper keeps a route orthogonal after a
      // node move by sliding the endpoint-adjacent waypoints onto the ENDPOINTS'
      // own coordinates — right when an edge is pinned to side centres, wrong for
      // an anchored one: it drags the first waypoint back onto the source's x and
      // the last onto the target's x, so the wire detours around the two legs it
      // no longer needs (a jog at each end, and the arrowhead behind a spur).
      // `enforcePerpendicularEnds` does the same repair anchor-aware, and it is
      // what keeps the legs square after a move.
      const interior = waypoints.map((w) => ({ x: w.x, y: w.y }));
      return enforcePerpendicularEnds(
        [sourcePt, ...interior, targetPt],
        srcSide,
        tgtSide,
        leg,
        tgtRect ?? undefined,
      );
    }
    // « Re-route automatically » produces a grid-ALIGNED auto path (#844): the
    // router's bends are snapped onto the wiring lattice so an auto edge and a
    // hand-drawn one line up instead of missing by a few pixels. It lands
    // perpendicular too, so the reset does not undo the arrowhead's orientation.
    const auto = routeOrthogonal({
      source: sourcePt,
      target: targetPt,
      obstacles,
      targetSide: data?.targetSide,
    });
    // `manual: false`: the router's bends are not the user's pins, so a wire that
    // has to go around its target is re-laid from its last real corner.
    return enforcePerpendicularEnds(
      snapPolyline(auto, CANVAS_LATTICE, WIRING_GRID_STEP),
      srcSide,
      tgtSide,
      leg,
      tgtRect ?? undefined,
      { manual: false },
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode, waypoints, sourcePt.x, sourcePt.y, targetPt.x, targetPt.y, obstacles, data?.targetSide, srcSide, tgtSide, leg, tgtRect]);

  const d = pathToSvg(points);
  // The first and last segments are the perpendicular legs into the two anchors:
  // structural, not user-shapable. They carry no drag handle.
  //
  // This is not only tidiness. Dragging the source leg made `dragSegmentOnGrid`
  // insert a bend beside the pinned endpoint; the leg was then re-imposed on
  // render, leaving that bend stranded beside it and the wire detouring left and
  // back right around a leg it no longer needed. Move the leg where it lands
  // instead — that is what the anchor is for.
  const handles = useMemo(
    () =>
      segmentHandles(points).filter(
        (h) => h.segmentIndex !== 0 && h.segmentIndex !== points.length - 2,
      ),
    [points],
  );

  const edgeIndex = data?.edgeIndex;

  // Shift state during a segment drag, read off the live event (the pointer is
  // captured, so a keyboard listener would be the only other way in).
  const shiftRef = useRef(false);

  const onHandleDrag = useCallback(
    (segmentIndex: number, orientation: "horizontal" | "vertical") =>
      (e: React.PointerEvent) => {
        e.stopPropagation();
        // Primary button only. Without this, a right-click on a handle still
        // starts a drag and silently reshapes the route — the delete-waypoint
        // gesture is meant to be GONE (#844), not renamed.
        if (e.button !== 0) return;
        if (edgeIndex == null) return;
        // The session origin is read ONCE, at grab time: a segment drag belongs
        // to the gesture in progress (a Shift release inside it re-origins), and
        // freezing it here keeps the snap stable for the whole drag.
        const origin = useWiringStore.getState().origin;
        let latest = points;
        let accepted = false;
        const move = (ev: PointerEvent) => {
          shiftRef.current = ev.shiftKey;
          const flow = screenToFlowPosition({ x: ev.clientX, y: ev.clientY });
          const coord = orientation === "horizontal" ? flow.y : flow.x;
          // Only THIS segment moves. `dragSegmentOnGrid` never rewrites the
          // neighbours' coordinates, so the rest of the route is byte-stable.
          const next = dragSegmentKeepingRun(
            points,
            segmentIndex,
            coord,
            { origin, step: WIRING_GRID_STEP, free: ev.shiftKey },
            (pts) => enforcePerpendicularEnds(pts, srcSide, tgtSide, leg, tgtRect ?? undefined),
            tgtRect ?? undefined,
          );
          // A position the route cannot keep the run at (across the target
          // card): hold the last one it could, instead of drawing the wire
          // jumping away from the pointer.
          if (!next.keepsRun && accepted) return;
          accepted = true;
          latest = next.moved;
          const enforced = next.enforced;
          updateEdge(edgeIndex, {
            mode: "manual",
            waypoints: storableWaypoints(enforced).map((p) => ({
              x: Math.round(p.x),
              y: Math.round(p.y),
            })),
          });
        };
        const up = () => {
          window.removeEventListener("pointermove", move);
          window.removeEventListener("pointerup", up);
          // Releasing Shift at the end of the drag re-origins the grid on the
          // point just placed (ADR-0072) — the same semantics as while wiring.
          if (shiftRef.current) {
            const tip = latest[Math.min(segmentIndex + 1, latest.length - 1)];
            if (tip) useWiringStore.getState().setOrigin({ x: tip.x, y: tip.y });
            shiftRef.current = false;
          }
          // Aligning two segments merges the waypoint between them (#844). That is
          // the ONLY way to delete a waypoint now.
          const merged = enforcePerpendicularEnds(
            mergeAlignedSegments(latest),
            srcSide,
            tgtSide,
            leg,
            tgtRect ?? undefined,
          );
          const interior = storableWaypoints(merged);
          updateEdge(edgeIndex, {
            mode: interior.length > 0 ? "manual" : "auto",
            waypoints:
              interior.length > 0
                ? interior.map((p) => ({ x: Math.round(p.x), y: Math.round(p.y) }))
                : null,
          });
        };
        window.addEventListener("pointermove", move);
        window.addEventListener("pointerup", up);
      },
    [edgeIndex, points, screenToFlowPosition, updateEdge, srcSide, tgtSide, leg, tgtRect],
  );

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
        if (moved) commit(screenToFlowPosition({ x: ev.clientX, y: ev.clientY }));
      };
      const up = () => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", up);
        if (!moved) setSelection({ kind: "edge", id: null, edgeIndex });
      };
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", up);
    },
    [edgeIndex, screenToFlowPosition, setSelection],
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
  const strokeColor = isSelected
    ? "var(--color-edge-selected, #fdba74)"
    : data?.strokeColor ?? "var(--color-fg-4)";

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
          strokeDasharray: data?.dashed ? "6 3" : undefined,
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
                  fontFamily: "var(--font-mono, monospace)",
                  fontSize: 9.5,
                  lineHeight: 1.3,
                  whiteSpace: "nowrap",
                  cursor: "grab",
                  color: "var(--color-fg-2)",
                  background: "var(--color-bg-3)",
                  border: "1px solid var(--color-line)",
                  borderRadius: 2,
                  padding: "1px 4px",
                }}
              >
                {port}
              </div>
            );
          })}
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
              fontFamily: "var(--font-mono, monospace)",
              fontSize: 10,
              color: "var(--color-fg)",
              background: "var(--color-bg-2, #1e1e1e)",
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
