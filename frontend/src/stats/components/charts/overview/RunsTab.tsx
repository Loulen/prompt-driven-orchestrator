import {
  Bar,
  BarChart,
  CartesianGrid,
  Legend,
  Tooltip as RTooltip,
  XAxis,
  YAxis,
} from "recharts";
import type { StatsOverview } from "../../../../shared/generated/StatsOverview";
import { Tooltip } from "../../../../shared/ui/tooltip";
import { ChartFrame, EmptyNote } from "../common/ChartPrimitives";
import { AXIS_PROPS, CHART } from "../common/chartTheme";

export function RunsTab({
  overview,
  completedOnly,
}: {
  overview: StatsOverview;
  completedOnly: boolean;
}) {
  if (overview.buckets.length === 0)
    return <EmptyNote>No runs in this period.</EmptyNote>;
  const runs = new Map(overview.runs.map((row) => [row.bucket, row.count]));
  const errors = new Map(overview.errors.map((row) => [row.bucket, row.count]));
  const data = overview.buckets.map((bucket) => ({
    bucket,
    runs: runs.get(bucket) ?? 0,
    errors: errors.get(bucket) ?? 0,
  }));
  const totalErrors = overview.errors.reduce((sum, row) => sum + row.count, 0);
  return (
    <div data-testid="stats-chart-runs">
      {completedOnly && (
        // #810: the card stays, at zero — nothing is masked. A completed Run
        // carries no `run_failed` of its own, so the series is 0 by
        // construction; a Run that failed, was reopened and then completed is
        // the one shape that can still count.
        <>
          <div className="mb-3" style={{ fontSize: "11px" }}>
            <Tooltip
              content="0 by construction: a completed run has no run_failed of its own."
              side="top"
            >
              <span
                className="rounded bg-surface-2 px-2 py-1 text-text-2"
                data-testid="stats-kpi-errors-completed-only"
              >
                Errors: <span className="font-mono text-text">{totalErrors}</span>{" "}
                (completed only)
              </span>
            </Tooltip>
          </div>
        </>
      )}
      <ChartFrame>
        <BarChart data={data} margin={{ top: 8, right: 8, left: -18 }}>
          <CartesianGrid
            stroke={CHART.grid}
            strokeDasharray="3 3"
            vertical={false}
          />
          <XAxis dataKey="bucket" {...AXIS_PROPS} />
          <YAxis allowDecimals={false} {...AXIS_PROPS} />
          <RTooltip
            contentStyle={{
              background: CHART.tooltipBg,
              border: `1px solid ${CHART.grid}`,
              fontSize: 11,
            }}
          />
          <Legend wrapperStyle={{ fontSize: 11 }} />
          <Bar dataKey="runs" name="Runs" fill={CHART.runs} />
          <Bar dataKey="errors" name="Errors (failed)" fill={CHART.errors} />
        </BarChart>
      </ChartFrame>
    </div>
  );
}
