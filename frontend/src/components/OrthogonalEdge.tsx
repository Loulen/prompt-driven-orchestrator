import { useMemo, useState, useCallback, type CSSProperties } from "react";
import {
  BaseEdge,
  EdgeLabelRenderer,
  useStore,
  useReactFlow,
  type EdgeProps,
  type Edge,
} from "@xyflow/react";
import { routeOrthogonal, type Point, type Rect } from "../lib/orthogonalRouter";
import { pathToSvg, segmentHandles, dragSegment } from "../lib/edgePath";
import { useEditStore } from "../stores/editStore";
import type { EdgeWaypoint } from "../types";

export interface OrthogonalEdgeData extends Record<string, unknown> {
  edgeIndex: number;
  mode?: "auto" | "manual" | null;
  waypoints?: EdgeWaypoint[] | null;
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
      // Pinned route: endpoints follow the (re-anchorable) handles, the
      // interior follows the persisted absolute waypoints.
      return [sourcePt, ...waypoints.map((w) => ({ x: w.x, y: w.y })), targetPt];
    }
    return routeOrthogonal({ source: sourcePt, target: targetPt, obstacles });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode, waypoints, sourceX, sourceY, targetX, targetY, obstacles]);

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

  const strokeColor = data?.strokeColor ?? "var(--color-fg-4)";

  const labelPoint = points[Math.floor(points.length / 2)] ?? targetPt;

  return (
    <>
      <BaseEdge
        id={id}
        path={d}
        markerEnd={markerEnd}
        style={{
          stroke: strokeColor,
          strokeWidth: 1.5,
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
              onMouseEnter={() => setHovered(true)}
              style={segHandleStyle(h.x, h.y, h.orientation)}
            />
          ))}
      </EdgeLabelRenderer>
    </>
  );
}

function segHandleStyle(
  x: number,
  y: number,
  orientation: "horizontal" | "vertical",
): CSSProperties {
  // A horizontal segment is dragged vertically (ns-resize) and vice-versa.
  return {
    position: "absolute",
    transform: `translate(-50%, -50%) translate(${x}px, ${y}px)`,
    width: orientation === "horizontal" ? 18 : 8,
    height: orientation === "horizontal" ? 8 : 18,
    borderRadius: 3,
    background: "var(--color-acc, #10b981)",
    opacity: 0.85,
    cursor: orientation === "horizontal" ? "ns-resize" : "ew-resize",
    pointerEvents: "all",
  };
}
