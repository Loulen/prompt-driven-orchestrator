import { Bar, BarChart, Tooltip as RTooltip, XAxis, YAxis, CartesianGrid } from "recharts";
import type { StatsOverview } from "../../../../shared/generated/StatsOverview";
import {
  ChartFrame,
  ChartTooltip,
  EmptyNote,
  Kpis,
  Panel,
} from "../common/ChartPrimitives";
import { AXIS_PROPS, CHART } from "../common/chartTheme";

/** A pipeline name on the x axis, cut so long names never overlap. */
const shortName = (name: string) => (name.length > 18 ? `${name.slice(0, 17)}…` : name);

export function TriggersTab({ overview }: { overview: StatsOverview }) {
  const kpi = overview.triggers_created_runs;
  const fires = overview.fires_by_pipeline.reduce((sum, fire) => sum + fire.count, 0);
  return (
    <div data-testid="stats-chart-triggers" className="flex flex-col gap-4">
      <Kpis
        items={[
          {
            key: "created",
            label: "Fires that created a run:",
            value: kpi.fired,
            testid: "stats-kpi-created-runs",
          },
          {
            key: "distinct",
            label: "Triggers fired",
            value: (
              <>
                {kpi.distinct_triggers} of {kpi.enabled_triggers}
              </>
            ),
            sub: "enabled triggers fired",
            testid: "stats-kpi-distinct",
          },
          { key: "fires", label: "Fires", value: fires },
        ]}
      />
      <Panel title="Fires by pipeline" count={overview.fires_by_pipeline.length}>
        {overview.fires_by_pipeline.length === 0 ? (
          <EmptyNote>No trigger fires in this period.</EmptyNote>
        ) : (
          <ChartFrame>
            <BarChart
              data={overview.fires_by_pipeline.map((fire) => ({
                ...fire,
                bucket: fire.name ?? fire.pipeline_id,
              }))}
              margin={{ top: 8, right: 8, left: 0, bottom: 0 }}
            >
              <CartesianGrid stroke={CHART.grid} strokeDasharray="2 3" vertical={false} />
              <XAxis dataKey="bucket" {...AXIS_PROPS} tickFormatter={shortName} interval={0} />
              <YAxis allowDecimals={false} {...AXIS_PROPS} width={44} />
              <RTooltip cursor={{ fill: CHART.cursor }} content={<ChartTooltip />} />
              <Bar dataKey="count" name="Fires" fill={CHART.fires} maxBarSize={48} isAnimationActive={false} />
            </BarChart>
          </ChartFrame>
        )}
      </Panel>
    </div>
  );
}
