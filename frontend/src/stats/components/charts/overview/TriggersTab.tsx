import {
  Bar,
  BarChart,
  CartesianGrid,
  Tooltip as RTooltip,
  XAxis,
  YAxis,
} from "recharts";
import type { StatsOverview } from "../../../../shared/generated/StatsOverview";
import { ChartFrame, EmptyNote } from "../common/ChartPrimitives";
import { AXIS_PROPS, CHART } from "../common/chartTheme";

export function TriggersTab({ overview }: { overview: StatsOverview }) {
  const kpi = overview.triggers_created_runs;
  return (
    <div data-testid="stats-chart-triggers" className="flex flex-col gap-3">
      <div className="flex flex-wrap gap-2" style={{ fontSize: "11px" }}>
        <span
          className="rounded bg-surface-2 px-2 py-1 text-text-2"
          data-testid="stats-kpi-created-runs"
        >
          Fires that created a run:{" "}
          <span className="font-mono text-text">{kpi.fired}</span>
        </span>
        <span
          className="rounded bg-surface-2 px-2 py-1 text-text-2"
          data-testid="stats-kpi-distinct"
        >
          <span className="font-mono text-text">{kpi.distinct_triggers}</span> of{" "}
          <span className="font-mono text-text">{kpi.enabled_triggers}</span>{" "}
          enabled triggers fired
        </span>
      </div>
      {overview.fires_by_pipeline.length === 0 ? (
        <EmptyNote>No trigger fires in this period.</EmptyNote>
      ) : (
        <ChartFrame>
          <BarChart
            data={overview.fires_by_pipeline.map((fire) => ({
              ...fire,
              label: fire.name ?? fire.pipeline_id,
            }))}
          >
            <CartesianGrid
              stroke={CHART.grid}
              strokeDasharray="3 3"
              vertical={false}
            />
            <XAxis dataKey="label" {...AXIS_PROPS} />
            <YAxis allowDecimals={false} {...AXIS_PROPS} />
            <RTooltip
              contentStyle={{
                background: CHART.tooltipBg,
                border: `1px solid ${CHART.grid}`,
                fontSize: 11,
              }}
            />
            <Bar dataKey="count" name="Fires" fill={CHART.fires} />
          </BarChart>
        </ChartFrame>
      )}
    </div>
  );
}
