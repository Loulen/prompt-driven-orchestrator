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
    return cssColor("--color-chart-runs", "#58a6ff");
  },
  get errors() {
    return cssColor("--color-chart-errors", "#f85149");
  },
  get fires() {
    return cssColor("--color-chart-fires", "#3fb950");
  },
  get grid() {
    return cssColor("--color-chart-grid", "#30363d");
  },
  get axis() {
    return cssColor("--color-chart-axis", "#8b949e");
  },
  get tooltipBg() {
    return cssColor("--color-chart-tooltip-bg", "#161b22");
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
