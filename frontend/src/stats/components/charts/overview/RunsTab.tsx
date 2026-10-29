import { Bar, BarChart, Tooltip as RTooltip } from "recharts";
import type { StatsOverview } from "../../../../shared/generated/StatsOverview";
import {
  ChartFrame,
  ChartLegend,
  ChartTooltip,
  EmptyNote,
  Kpis,
  Panel,
} from "../common/ChartPrimitives";
import { chartAxes } from "../common/chartAxes";
import { CHART } from "../common/chartTheme";

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
  // Stacked (`overlays.statsChart`): the failed Runs on top of the others, so
  // the column reads the period's Runs.
  const data = overview.buckets.map((bucket) => {
    const total = runs.get(bucket) ?? 0;
    const failed = errors.get(bucket) ?? 0;
    return { bucket, other: Math.max(0, total - failed), failed };
  });
  const totalRuns = overview.runs.reduce((sum, row) => sum + row.count, 0);
  const totalErrors = overview.errors.reduce((sum, row) => sum + row.count, 0);
  const notFailed = data.reduce((sum, row) => sum + row.other, 0);
  const busiest = Math.max(0, ...overview.runs.map((row) => row.count));
  // The buckets are days up to 30 days, then weeks or months.
  const bucketWord = /^\d{4}-\d{2}-\d{2}$/.test(overview.buckets[0] ?? "") ? "day" : "period";
  return (
    <div className="flex flex-col gap-4" data-testid="stats-chart-runs">
      <Kpis
        items={[
          { key: "runs", label: "Runs", value: totalRuns },
          completedOnly
            ? {
                // #810: the figure stays, at zero — nothing is masked. A
                // completed Run carries no `run_failed` of its own, so the
                // series is 0 by construction; a Run that failed, was reopened
                // and then completed is the one shape that can still count.
                key: "errors",
                label: "Errors:",
                value: totalErrors,
                sub: "completed only",
                testid: "stats-kpi-errors-completed-only",
                title: "0 by construction: a completed run has no run_failed of its own.",
              }
            : { key: "errors", label: "Failed", value: totalErrors },
          {
            key: "rate",
            label: "Failure rate",
            // `run_failed` events: a Run reopened and failed again counts twice,
            // so the rate is capped where it stops meaning anything.
            value: totalRuns ? `${Math.min(100, Math.round((totalErrors / totalRuns) * 100))}%` : "—",
          },
          { key: "busiest", label: `Busiest ${bucketWord}`, value: busiest },
        ]}
      />
      <Panel
        title="Runs by outcome"
        aside={
          <ChartLegend
            items={[
              { key: "other", label: "Not failed", color: CHART.runs, total: String(notFailed) },
              { key: "failed", label: "Failed", color: CHART.errors, total: String(totalErrors) },
            ]}
          />
        }
      >
        <ChartFrame>
          <BarChart data={data} margin={{ top: 8, right: 8, left: 0, bottom: 0 }}>
            {chartAxes(false)}
            <RTooltip cursor={{ fill: CHART.cursor }} content={<ChartTooltip />} />
            <Bar dataKey="other" name="Not failed" stackId="runs" fill={CHART.runs} maxBarSize={28} isAnimationActive={false} />
            <Bar dataKey="failed" name="Failed" stackId="runs" fill={CHART.errors} maxBarSize={28} isAnimationActive={false} />
          </BarChart>
        </ChartFrame>
      </Panel>
    </div>
  );
}
