/**
 * The widths of the Workbench's panes: the left panel (Runs, Triggers,
 * Pipelines) opens at 260 CSS px, the inspector at 400, the canvas takes the
 * rest; the panel library takes them as shares of the window. A drag stays
 * within `PANE_BOUNDS`. A saved layout keeps the side panels' pixels in
 * another window or at another UI scale.
 */

const LEFT_PX = 260;
const RIGHT_PX = 400;
/** The canvas keeps at least this share of a narrow window. */
const CENTER_MIN_PCT = 34;

export const PANE_BOUNDS = {
  left: { min: 200, max: 480 },
  right: { min: 240, max: 720 },
} as const;

export type PaneSizes = {
  left: number;
  center: number;
  right: number;
};

/** Side panels `leftPx` and `rightPx` CSS px wide, as shares (in %) of a window `width` CSS px wide. */
function shares(width: number, leftPx: number, rightPx: number): PaneSizes {
  let left = (leftPx / width) * 100;
  let right = (rightPx / width) * 100;
  const sides = 100 - CENTER_MIN_PCT;
  if (left + right > sides) {
    const scale = sides / (left + right);
    left *= scale;
    right *= scale;
  }
  left = +left.toFixed(2);
  right = +right.toFixed(2);
  return { left, center: +(100 - left - right).toFixed(2), right };
}

const clamp = (px: number, { min, max }: { min: number; max: number }) => Math.min(max, Math.max(min, px));

/** The default shares (in %) of the three panes for a window `width` CSS px wide. */
export function defaultPaneSizes(width: number): PaneSizes {
  if (!(width > 0)) return { left: 20, center: 55, right: 25 };
  return shares(width, LEFT_PX, RIGHT_PX);
}

/**
 * The shares of a layout saved in a window `savedWidth` CSS px wide, for a
 * window `width` CSS px wide: the side panels keep their pixels, within
 * `PANE_BOUNDS`. A layout saved without its width (an older one) gives way to
 * the defaults.
 */
export function restorePaneSizes(saved: PaneSizes, savedWidth: number | null, width: number): PaneSizes {
  if (!(width > 0) || savedWidth == null || !(savedWidth > 0)) return defaultPaneSizes(width);
  const px = (pct: number) => (pct / 100) * savedWidth;
  return shares(width, clamp(px(saved.left), PANE_BOUNDS.left), clamp(px(saved.right), PANE_BOUNDS.right));
}
