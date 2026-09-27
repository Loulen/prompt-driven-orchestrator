import {
  Bar,
  BarChart,
  CartesianGrid,
  Legend,
  ResponsiveContainer,
  Tooltip as RTooltip,
  XAxis,
  YAxis,
} from "recharts";
import type { StatsCostPeriod } from "../../../../shared/generated/StatsCostPeriod";
import type { StatsSessionPeriod } from "../../../../shared/generated/StatsSessionPeriod";
import { formatCostAmount } from "../../../../run/model/costLabel";
import { harnessColor } from "../../../../harness/model/color";
import { flattenPeriods, isHarnessCost } from "../../../model/chartData";
import { AXIS_PROPS, CHART } from "./chartTheme";

export function ChartFrame({ children }: { children: React.ReactElement }) {
  return (
    <div style={{ width: "100%", height: 220 }}>
      <ResponsiveContainer width="100%" height="100%">
        {children}
      </ResponsiveContainer>
    </div>
  );
}

export function EmptyNote({ children }: { children: React.ReactNode }) {
  return (
    <div
      className="px-1 py-8 text-center text-fg-4"
      style={{ fontSize: "11.5px" }}
    >
      {children}
    </div>
  );
}

/** Which Runs the figures on screen describe (#810). Always visible, never a
 *  silent filter: amber once « completed runs only » narrows the cohort, muted
 *  otherwise, and explicit about what was left out. */
export function CohortLine({
  completedOnly,
  note,
}: {
  completedOnly: boolean;
  /** An extra clause for a tab the cohort does not govern. */
  note?: string;
}) {
  return (
    <div
      data-testid="stats-cohort-line"
      className={completedOnly ? "text-st-await" : "text-fg-4"}
      style={{ fontSize: "10.5px" }}
    >
      Cohort: runs started in the period
      {completedOnly ? (
        <>
          {" · "}
          <span className="font-medium">completed runs only</span> (failed,
          stopped and running runs left out)
        </>
      ) : null}
      {note ? ` · ${note}` : ""}
    </div>
  );
}

export function HarnessLegend({ harnesses }: { harnesses: string[] }) {
  return (
    <div
      className="flex flex-wrap gap-3 text-fg-3"
      style={{ fontSize: "10.5px" }}
    >
      {harnesses.map((harness) => (
        <span key={harness} className="flex items-center gap-1.5">
          <span
            className="h-2 w-2 rounded-full"
            style={{ backgroundColor: harnessColor(harness) }}
            data-testid={`stats-harness-legend-${harness}`}
          />
          {harness}
        </span>
      ))}
    </div>
  );
}

export function HarnessBars({
  periods,
  harnesses,
  value,
}: {
  periods: (StatsSessionPeriod | StatsCostPeriod)[];
  harnesses: string[];
  value: "executions" | "usd";
}) {
  if (periods.length === 0)
    return <EmptyNote>No activity in this period.</EmptyNote>;
  return (
    <ChartFrame>
      <BarChart
        data={flattenPeriods(periods, value)}
        margin={{ top: 8, right: 8, left: -12 }}
      >
        <CartesianGrid
          stroke={CHART.grid}
          strokeDasharray="3 3"
          vertical={false}
        />
        <XAxis dataKey="bucket" {...AXIS_PROPS} />
        <YAxis allowDecimals={value === "usd"} {...AXIS_PROPS} />
        <RTooltip
          contentStyle={{
            background: CHART.tooltipBg,
            border: `1px solid ${CHART.grid}`,
            fontSize: 11,
          }}
          formatter={(raw, name) => {
            if (raw == null) return ["—", String(name)];
            const amount = typeof raw === "number" ? raw : Number(raw);
            const metric = periods
              .flatMap((period) => period.harnesses)
              .find((harness) => harness.harness === String(name));
            const costMetric = isHarnessCost(metric) ? metric : undefined;
            const label =
              value === "usd"
                ? formatCostAmount(
                    amount,
                    costMetric?.partial ?? false,
                    costMetric?.estimated ?? true,
                  )
                : amount;
            return [label, String(name)];
          }}
        />
        <Legend wrapperStyle={{ fontSize: 11 }} />
        {harnesses.map((harness) => (
          <Bar
            key={harness}
            dataKey={harness}
            name={harness}
            stackId="harness"
            fill={harnessColor(harness)}
          />
        ))}
      </BarChart>
    </ChartFrame>
  );
}
