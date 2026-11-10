import { cssColor } from "../../../../lib/cssColor";

/**
 * Chart colours (#759). Getters, not constants: recharts takes resolved colours
 * as props, so each read must happen at RENDER time against the live palette.
 * `StatsCharts` subscribes to the theme so a switch re-renders the subtree and
 * these are read again. The type of the ticks comes from the sheet
 * (`.sx-chart`), never from a prop.
 */
export const CHART = {
  get runs() {
    return cssColor("--color-st-running");
  },
  get errors() {
    return cssColor("--color-st-failed");
  },
  get fires() {
    return cssColor("--color-st-done");
  },
  get grid() {
    return cssColor("--line-soft");
  },
  get axis() {
    return cssColor("--line");
  },
  get cursor() {
    return cssColor("--hover");
  },
};

export const AXIS_PROPS = {
  get stroke() {
    return CHART.axis;
  },
  tickLine: false,
};

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** A day bucket (`2026-10-01`) as « 1 Oct »; a week or a month bucket as is. */
export function bucketLabel(bucket: string): string {
  const day = /^(\d{4})-(\d{2})-(\d{2})$/.exec(bucket);
  if (!day) return bucket;
  return `${Number(day[3])} ${MONTHS[Number(day[2]) - 1] ?? day[2]}`;
}
