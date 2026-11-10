import { CartesianGrid, XAxis, YAxis } from "recharts";
import { AXIS_PROPS, CHART, bucketLabel } from "./chartTheme";

/** The bars' common grid and axes: hairlines, mono ticks, dates as « 1 Oct ». */
export function chartAxes(allowDecimals: boolean) {
  return [
    <CartesianGrid key="grid" stroke={CHART.grid} strokeDasharray="2 3" vertical={false} />,
    <XAxis key="x" dataKey="bucket" {...AXIS_PROPS} tickFormatter={bucketLabel} minTickGap={16} />,
    <YAxis key="y" allowDecimals={allowDecimals} {...AXIS_PROPS} width={44} />,
  ];
}

