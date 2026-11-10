import { createContext, useContext } from "react";
import type { EdgeDef } from "../../shared/generated/EdgeDef";
import type { EditorPipelineDef } from "../model/pipeline";

/**
 * How the canvas draws the pipeline against how the file stores it, for the
 * gestures that write a position: the edit canvas draws a card with several
 * outputs taller and moves what lies below down by as much
 * (`model/grownCanvas.ts`), and a position written back must be the stored
 * one. Identity when nothing is shifted.
 */
export interface CanvasShift {
  /** The pipeline as drawn. */
  shown?: EditorPipelineDef;
  /** Where a point drawn at `y` is stored; `moving` leaves those boxes out of the layout. */
  toStored: (y: number, moving?: ReadonlySet<string>) => number;
}

export const IDENTITY_SHIFT: CanvasShift = { toStored: (y) => y };

export const CanvasShiftContext = createContext<CanvasShift>(IDENTITY_SHIFT);

/** The canvas's shift, for an edge's segment or label drag. */
export function useCanvasShift(): CanvasShift {
  return useContext(CanvasShiftContext);
}

/** A drawn point, as the file stores it. */
export function storedPoint<P extends { x: number; y: number }>(shift: CanvasShift, p: P): P {
  return { ...p, y: Math.round(shift.toStored(p.y)) };
}

/** The positions of an edge update (waypoints, pinned labels), drawn, as the file stores them. */
export function storedEdgePatch<U extends Partial<EdgeDef>>(shift: CanvasShift, updates: U): U {
  if (shift === IDENTITY_SHIFT) return updates;
  const out: U = { ...updates };
  if (updates.waypoints) out.waypoints = updates.waypoints.map((p) => storedPoint(shift, p));
  if (updates.condition_label_pos) out.condition_label_pos = storedPoint(shift, updates.condition_label_pos);
  if (updates.output_label_pos)
    out.output_label_pos = Object.fromEntries(
      Object.entries(updates.output_label_pos).map(([port, at]) => [port, storedPoint(shift, at)]),
    );
  return out;
}
