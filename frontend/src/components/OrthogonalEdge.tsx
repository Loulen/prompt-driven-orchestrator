import { useMemo, useState, useCallback } from "react";
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
  segmentHandles,
  dragSegment,
  reanchorWaypoints,
  segHandleStyle,
  deleteWaypoint,
} from "../lib/edgePath";
import { useEditStore } from "../stores/editStore";
import type { EdgeWaypoint, PortSide } from "../types";

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
  isConditional: boolean;
  isElse: boolean;
  label?: string;
  strokeColor: string;
  dashed: boolean;
}

/**
 * Orthogonal (right-angle) edge with manual-waypoint shaping (#154, design
 * screen 14). Auto edges pathfind around other nodes via `routeOrthogonal` and
 * re-route for free when a node moves (the path is recomputed every render from
 * live node positions). Hovering reveals perpendicular-only segment handles;
 * the first drag pins the route to persisted `manual` waypoints. The reset back
 * to auto lives in the edge detail panel.
 */
export default function OrthogonalEdge({
  id,
  sourceX,
  sourceY,
  targetX,
  targetY,
  source,
  target,
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
  const { screenToFlowPosition } = useReactFlow();
  const [hovered, setHovered] = useState(false);

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

  const sourcePt: Point = { x: sourceX, y: sourceY };
  const targetPt: Point = { x: targetX, y: targetY };
  const mode = data?.mode;
  const waypoints = data?.waypoints;

  const points: Point[] = useMemo(() => {
    if (mode === "manual" && waypoints && waypoints.length > 0) {
      // Pinned route: endpoints follow their nodes, the interior follows the
      // persisted absolute waypoints. Re-anchor the endpoint-adjacent waypoints
      // against the live endpoints so every segment stays axis-aligned when a
      // connected node moves (#165) — and the pill, keyed off the midpoint,
      // stays centered on the re-routed path.
      const anchored = reanchorWaypoints(
        sourcePt,
        targetPt,
        waypoints.map((w) => ({ x: w.x, y: w.y })),
      );
      return [sourcePt, ...anchored, targetPt];
    }
    // Auto route: arrive from the persisted anchor side (#168 / #175) rather
    // than always horizontally from the left. Manual routes (above) arrive
    // however the user shaped their waypoints, so this only steers auto edges.
    return routeOrthogonal({ source: sourcePt, target: targetPt, obstacles, targetSide: data?.targetSide });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode, waypoints, sourceX, sourceY, targetX, targetY, obstacles, data?.targetSide]);

  const d = pathToSvg(points);
  const handles = useMemo(() => segmentHandles(points), [points]);

  const edgeIndex = data?.edgeIndex;

  const onHandleDrag = useCallback(
    (segmentIndex: number, orientation: "horizontal" | "vertical") =>
      (e: React.PointerEvent) => {
        e.stopPropagation();
        if (edgeIndex == null) return;
        const move = (ev: PointerEvent) => {
          const flow = screenToFlowPosition({ x: ev.clientX, y: ev.clientY });
          const coord = orientation === "horizontal" ? flow.y : flow.x;
          const next = dragSegment(points, segmentIndex, coord);
          // Pin: the interior points become the persisted waypoints (absolute).
          const interior = next.slice(1, next.length - 1);
          updateEdge(edgeIndex, {
            mode: "manual",
            waypoints: interior.map((p) => ({ x: Math.round(p.x), y: Math.round(p.y) })),
          });
        };
        const up = () => {
          window.removeEventListener("pointermove", move);
          window.removeEventListener("pointerup", up);
        };
        window.addEventListener("pointermove", move);
        window.addEventListener("pointerup", up);
      },
    [edgeIndex, points, screenToFlowPosition, updateEdge],
  );

  // Right-click a segment handle to delete the waypoint it sits on (#169). The
  // interior `points` ARE the persisted waypoints (point index k ⇒ waypoint
  // index k-1). A handle on segment `i` spans points[i]..points[i+1]; drop the
  // interior point it touches (prefer the segment start when it is a waypoint,
  // else the segment end). Removing the last waypoint reverts the edge to auto.
  const onHandleDelete = useCallback(
    (segmentIndex: number) => (e: React.MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (edgeIndex == null || mode !== "manual" || !waypoints || waypoints.length === 0) {
        return;
      }
      const lastIdx = points.length - 1;
      const startIsWaypoint = segmentIndex >= 1 && segmentIndex <= lastIdx - 1;
      const wpIndex = startIsWaypoint ? segmentIndex - 1 : segmentIndex;
      const next = deleteWaypoint(
        waypoints.map((w) => ({ x: w.x, y: w.y })),
        wpIndex,
      );
      if (next.length === 0) {
        updateEdge(edgeIndex, { mode: "auto", waypoints: null });
        return;
      }
      updateEdge(edgeIndex, {
        mode: "manual",
        waypoints: next.map((p) => ({ x: Math.round(p.x), y: Math.round(p.y) })),
      });
    },
    [edgeIndex, mode, waypoints, points.length, updateEdge],
  );

  // Pastel orange when this edge is the selected one, grey otherwise (#177).
  // The override flows through to the condition pill border and segment handles
  // below, which both read `strokeColor`, so the whole edge reads as selected.
  const isSelected = selection.kind === "edge" && selection.edgeIndex === edgeIndex;
  const strokeColor = isSelected
    ? "var(--color-edge-selected, #fdba74)"
    : data?.strokeColor ?? "var(--color-fg-4)";

  const labelPoint = points[Math.floor(points.length / 2)] ?? targetPt;

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
      {/* Wide invisible hit area to make hover/handles forgiving. */}
      <path
        d={d}
        fill="none"
        stroke="transparent"
        strokeWidth={14}
        style={{ pointerEvents: "stroke", cursor: "pointer" }}
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        data-testid={`orthogonal-edge-hit-${id}`}
      />
      <EdgeLabelRenderer>
        {/* Conditional pill (ADR-0011) — always visible, mirrors the prior edge. */}
        {data?.label && (
          <div
            className="nodrag nopan"
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
              pointerEvents: "none",
              whiteSpace: "nowrap",
            }}
          >
            {data.label}
          </div>
        )}
        {/* Perpendicular-only segment handles, revealed on hover. */}
        {(hovered || mode === "manual") &&
          handles.map((h) => (
            <div
              key={h.segmentIndex}
              className="nodrag nopan"
              data-testid={`edge-seg-handle-${id}-${h.segmentIndex}`}
              onPointerDown={onHandleDrag(h.segmentIndex, h.orientation)}
              onContextMenu={onHandleDelete(h.segmentIndex)}
              onMouseEnter={() => setHovered(true)}
              style={segHandleStyle(h.x, h.y, h.orientation, strokeColor)}
            />
          ))}
      </EdgeLabelRenderer>
    </>
  );
}
