import { useCallback, useMemo } from "react";
import { useStore } from "@xyflow/react";
import { routeOrthogonal, type Point, type Rect } from "../../lib/orthogonalRouter";
import { segmentHandles } from "../../lib/edgePath";
import { anchorPoint } from "../../lib/anchorSide";
import { enforcePerpendicularEnds, landingLeg } from "../../lib/anchorLanding";
import { snapPolyline } from "../../lib/wiringGrid";
import { useWiringGridStep } from "../../hooks/useWiringGrid";
import type { PortSide } from "../../shared/generated/PortSide";
import type { OrthogonalEdgeData } from "./OrthogonalEdge";

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

/** Where an edge runs: its endpoints, the sides it leaves and lands on, and the
 *  polyline between them, with the segment handles a selected edge shows. */
export interface EdgeRoute {
  points: Point[];
  handles: ReturnType<typeof segmentHandles>;
  targetPt: Point;
  srcSide: PortSide;
  tgtSide: PortSide;
  step: number;
  leg: number;
  tgtRect: Rect | null;
  /** Every card on the canvas, this edge's own two included. */
  cards: Rect[];
}

/**
 * The route of an {@link OrthogonalEdge}: a manual edge's stored waypoints with
 * its legs kept perpendicular, or an auto edge pathfound around the other nodes
 * and snapped onto the canvas lattice (#844). Re-computed when a node moves.
 */
export function useEdgeRoute(
  source: string,
  target: string,
  sourceX: number,
  sourceY: number,
  targetX: number,
  targetY: number,
  data: OrthogonalEdgeData | undefined,
): EdgeRoute {
  // Obstacle rects: every node except this edge's own source and target. Read
  // from the flow store so a node move re-renders the edge with fresh bounds.
  const obstacles = useStore(
    useCallback(
      (s): (Rect & { region: boolean })[] => {
        const rects: (Rect & { region: boolean })[] = [];
        for (const [, node] of s.nodeLookup) {
          if (node.id === source || node.id === target) continue;
          const w = node.measured?.width ?? node.width ?? 0;
          const h = node.measured?.height ?? node.height ?? 0;
          if (w === 0 || h === 0) continue;
          rects.push({
            x: node.internals.positionAbsolute.x,
            y: node.internals.positionAbsolute.y,
            width: w,
            height: h,
            region: node.type === "loopRegion",
          });
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
  // An edge without a source anchor leaves from the middle of its side, read off
  // the card — never off xyflow's handle position: the rim strip reaches outside
  // the card by a width that depends on the zoom (CORE-88), and the wire would
  // start further out the more one dezooms.
  const srcAnchored = srcRect
    ? anchorPoint(srcRect, sourceAnchor, sourceAnchor?.side ?? data?.sourceSide ?? "right")
    : null;
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
  // The active pipeline's grid step (#877): it sets the legs, the auto-route
  // snap and the segment-drag snap. Stored waypoints are never re-snapped by it.
  const step = useWiringGridStep();
  const leg = landingLeg(step);

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
      sourceSide: srcSide,
      sourceRect: srcRect ?? undefined,
      // Half a cell past the leg, so the lattice snap below never rounds the
      // router's first bend back inside the source leg.
      sourceStub: leg + step / 2,
    });
    // `manual: false`: the router's bends are not the user's pins, so a wire that
    // has to go around its target is re-laid from its last real corner, and one
    // that runs back through its own source card goes around it.
    return enforcePerpendicularEnds(
      snapPolyline(auto, CANVAS_LATTICE, step),
      srcSide,
      tgtSide,
      leg,
      tgtRect ?? undefined,
      { manual: false, sourceRect: srcRect ?? undefined },
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode, waypoints, sourcePt.x, sourcePt.y, targetPt.x, targetPt.y, obstacles, data?.targetSide, srcSide, tgtSide, step, leg, tgtRect, srcRect]);

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

  // The cards a label must not cover: every node but the loop regions, drawn
  // behind the cards and holding the edges themselves.
  const cards = useMemo(
    () => [...obstacles.filter((r) => !r.region), ...(srcRect ? [srcRect] : []), ...(tgtRect ? [tgtRect] : [])],
    [obstacles, srcRect, tgtRect],
  );

  return { points, handles, targetPt, srcSide, tgtSide, step, leg, tgtRect, cards };
}
