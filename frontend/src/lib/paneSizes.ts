/**
 * The widths of the Workbench's panes: the left panel (Runs, Triggers,
 * Pipelines) opens at 260 CSS px, the inspector at 400, the canvas takes the
 * rest; the panel library takes them as shares of the window. A drag stays
 * within `PANE_BOUNDS`.
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

/** The default shares (in %) of the three panes for a window `width` CSS px wide. */
export function defaultPaneSizes(width: number): PaneSizes {
  if (!(width > 0)) return { left: 20, center: 55, right: 25 };
  let left = (LEFT_PX / width) * 100;
  let right = (RIGHT_PX / width) * 100;
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
