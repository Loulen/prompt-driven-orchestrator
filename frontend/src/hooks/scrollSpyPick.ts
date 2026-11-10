/** A section's box, in the same coordinates as the view's. */
export interface SectionBox<Id extends string> {
  id: Id;
  top: number;
  bottom: number;
}

/**
 * The section a scroll-spy lights, `null` when there is none.
 *
 * - In the page: the heading nearest the top of the view, at or above its
 *   upper fifth; else the first one below.
 * - At the bottom, where the last headings can no longer reach the top: the
 *   section that fills most of the view, the later one on a tie. A long last
 *   section wins there (« the last section never highlights »), a short one
 *   under a long one does not.
 */
export function pickSection<Id extends string>(
  sections: readonly SectionBox<Id>[],
  view: { top: number; height: number },
  atBottom: boolean,
): Id | null {
  let best: Id | null = null;
  if (atBottom) {
    let most = -1;
    for (const s of sections) {
      const shown = Math.min(s.bottom, view.top + view.height) - Math.max(s.top, view.top);
      if (shown >= most) {
        most = shown;
        best = s.id;
      }
    }
    return best;
  }
  let bestScore = Number.POSITIVE_INFINITY;
  for (const s of sections) {
    const delta = s.top - view.top;
    const score = delta <= view.height * 0.2 ? Math.abs(delta) : delta + 1e6;
    if (score < bestScore) {
      bestScore = score;
      best = s.id;
    }
  }
  return best;
}
