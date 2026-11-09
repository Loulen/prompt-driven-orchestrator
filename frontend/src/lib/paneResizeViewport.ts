/**
 * Re-framing the canvas when the right pane folds or unfolds (#949, FP iteration 1).
 *
 * React Flow anchors its viewport on the canvas's top-left corner: when the
 * right pane unfolds, the canvas shrinks from the right and every card keeps its
 * screen position. The graph was fit-viewed on the full-width canvas (the pane is
 * collapsed when a pipeline opens), so its right end — and often the very card
 * the reader just clicked — slides under the pane.
 *
 * The answer, pure so it can be tested without a DOM: pan by half the width
 * change, so the graph keeps its visual centre, then — if a card is selected and
 * still not fully in frame — centre on it. Zoom never changes.
 *
 * The half-width pan assumes a centred graph. One the user panned to the left
 * would be pushed out past the canvas's left edge (FP iteration 2), so the pan
 * towards the left stops where the graph's left edge meets the canvas's — and
 * does not happen at all if that edge is already cut off.
 */

export interface Viewport {
  x: number;
  y: number;
  zoom: number;
}

/** A card's box in flow coordinates. */
export interface FlowRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export function reframeOnPaneResize(
  viewport: Viewport,
  prevWidth: number,
  width: number,
  height: number,
  selected: FlowRect | null,
  graph: FlowRect | null = null,
): Viewport {
  const { zoom } = viewport;
  let shift = (width - prevWidth) / 2;
  if (shift < 0 && graph) {
    const graphLeft = graph.x * zoom + viewport.x;
    shift = Math.max(shift, Math.min(0, -graphLeft));
  }
  const next = { x: viewport.x + shift, y: viewport.y, zoom };
  if (!selected) return next;
  const left = selected.x * zoom + next.x;
  const top = selected.y * zoom + next.y;
  const right = left + selected.width * zoom;
  const bottom = top + selected.height * zoom;
  const inFrame = left >= 0 && top >= 0 && right <= width && bottom <= height;
  if (inFrame) return next;
  return {
    x: width / 2 - (selected.x + selected.width / 2) * zoom,
    y: height / 2 - (selected.y + selected.height / 2) * zoom,
    zoom,
  };
}

/**
 * How long after the canvas mounts — or shows a tab for the first time — a width
 * change still counts as part of its opening (#43, FP iteration 1). Opening a run with no tab open mounts the
 * canvas in the same commit the run pane unfolds; React Flow's initial fit can
 * land on the full-width canvas before the pane takes its width, and half a
 * width of pan cannot shrink a graph fitted to a wider frame. Within the window
 * the view is fitted again instead, from the very first measure: React Flow can
 * measure (and fit) its nodes before its own width update in the same frame.
 */
export const MOUNT_FIT_WINDOW_MS = 1000;

export interface PaneResize {
  /** The canvas width at the previous measure, null on the first one. */
  prevWidth: number | null;
  width: number;
  msSinceMount: number;
  /** A fold or unfold of the right pane is waiting for its width change. */
  paneToggled: boolean;
  /** The view is still the last fit, unmoved by the reader. A graph fitted on
   *  the full-width canvas does not fit beside the pane at the same zoom, so a
   *  toggle fits it again rather than pan it (F3-4). */
  viewFitted?: boolean;
}

/** What the canvas does on a width change: fit again, re-frame, or nothing. */
export function paneResizeAction({ prevWidth, width, msSinceMount, paneToggled, viewFitted = false }: PaneResize): "fit" | "reframe" | null {
  if (width <= 0) return null;
  const opening = msSinceMount < MOUNT_FIT_WINDOW_MS;
  // FP iteration 2: the pane may have unfolded before the observer started, so
  // its first measure is the only one the opening produces — fit on it too.
  if (prevWidth == null) return opening ? "fit" : null;
  if (prevWidth <= 0 || width === prevWidth) return null;
  if (opening) return "fit";
  if (!paneToggled) return null;
  return viewFitted ? "fit" : "reframe";
}
