/** Gap between the caret and the list, and between the list and the viewport's edges. */
const GAP = 2;
const MARGIN = 4;

/** Where a fixed list opened at the caret goes: `top` under it, or `bottom` above it. */
export type CaretPlace =
  { top: number; left: number; maxHeight: number } | { bottom: number; left: number; maxHeight: number };

/**
 * Places a list at the caret (viewport coordinates): under it when its
 * natural `height` fits there, otherwise on the side with the most room, its
 * height capped to that room. Its left edge follows the caret, inside the
 * viewport.
 */
export function placeAtCaret(
  caret: { top: number; bottom: number; left: number },
  list: { width: number; height: number; maxHeight: number },
  viewport: { width: number; height: number },
): CaretPlace {
  const left = Math.max(MARGIN, Math.min(caret.left, viewport.width - list.width - MARGIN));
  const below = viewport.height - caret.bottom - GAP - MARGIN;
  const above = caret.top - GAP - MARGIN;
  const wanted = Math.min(list.height, list.maxHeight);
  if (wanted <= below || below >= above) {
    return {
      top: caret.bottom + GAP,
      left,
      maxHeight: Math.min(list.maxHeight, below),
    };
  }
  return {
    bottom: viewport.height - caret.top + GAP,
    left,
    maxHeight: Math.min(list.maxHeight, above),
  };
}
