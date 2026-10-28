import { cssColor } from "../../../../lib/cssColor";

/**
 * Chart colours (#759). Getters, not constants: recharts takes resolved colours
 * as props, so each read must happen at RENDER time against the live palette.
 * The call sites are unchanged — `CHART.grid` and `{...AXIS_PROPS}` now resolve
 * the token instead of returning a frozen hex. `StatsCharts` subscribes to the
 * theme so a switch re-renders the subtree and these are read again.
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
    return cssColor("--line");
  },
  get axis() {
    return cssColor("--text-3");
  },
  get tooltipBg() {
    return cssColor("--surface-2");
  },
};

export const AXIS_PROPS = {
  get stroke() {
    return CHART.axis;
  },
  get tick() {
    return { fill: CHART.axis, fontSize: 10 };
  },
};
