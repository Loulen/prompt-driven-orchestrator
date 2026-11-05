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
 * gitlab #23 « Show »: the viewport that centres `rect` on a canvas of
 * `width` × `height` at `zoom`. Computed from the canvas's measured size, not
 * React Flow's stored one, so it holds when the inspector unfolding has just
 * shrunk the canvas and React Flow has not caught up yet.
 *
 * `topInset` (#1022): the height the toolbar and the lint banner cover at the
 * top of the canvas — the card is centred in the band left visible below them,
 * so the banner never sits over the node « Show » just brought into view.
 */
export function centreOn(
  rect: FlowRect,
  width: number,
  height: number,
  zoom: number,
  topInset = 0,
): Viewport {
  const inset = Math.min(Math.max(topInset, 0), height);
  return {
    x: width / 2 - (rect.x + rect.width / 2) * zoom,
    y: inset + (height - inset) / 2 - (rect.y + rect.height / 2) * zoom,
    zoom,
  };
}
